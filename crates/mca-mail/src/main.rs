//! MCA Mail entry point.

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::time::Duration;

use tracing::{error, info};

/// How long the HTTP server may take to drain once a shutdown signal arrives.
/// The bound covers the drain only, never the time the server is allowed to
/// listen: compose allows 30s (`stop_grace_period`) for this plus the worker
/// stop below, so the process exits deliberately instead of being SIGKILLed
/// mid-request.
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(8);

/// How long the background workers may take to release their sessions after
/// the shutdown signal: the outbox must not be cut off mid-delivery and the
/// backup worker must not leave a partial dump behind.
const WORKER_STOP_DEADLINE: Duration = Duration::from_secs(15);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Structured logging: LOG_LEVEL / LOG_FORMAT=pretty|json (see observability).
    mca_mail::observability::init();

    // Any argument routes to the admin CLI (`mca-mail help`); no arguments
    // start the server.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        return mca_mail::cli::run(&args).await;
    }

    // Bootstrap all dependencies
    let app = match mca_mail::application::App::bootstrap().await {
        Ok(a) => a,
        Err(e) => {
            error!("Failed to bootstrap: {}", e);
            return Err(anyhow::anyhow!(e.to_string()));
        }
    };

    info!(
        env = %app.config.app.env,
        host = %app.config.api.host,
        port = %app.config.api.port,
        "MCA Mail starting"
    );

    // Start background workers (mail sync, IDLE, queue, backups, retention).
    let workers = mca_mail::application::workers::spawn_all(
        &app.config,
        app.pool.clone(),
        app.orchestrator.clone(),
        app.mailbox.clone(),
    )
    .await;

    // Build and start the HTTP server
    let router = mca_mail::api::build_router(&app.config.api, app.state.clone());

    let addr: SocketAddr = format!("{}:{}", app.config.api.host, app.config.api.port)
        .parse()
        .expect("valid socket address");

    info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // The same signal goes to the server and to every worker: the server stops
    // accepting connections first, then `stop` below waits for the loops to
    // release their sessions and finish what they were doing.
    //
    // The deadline must start at the signal, not at boot — wrapping the whole
    // `serve` future would kill a healthy process that is simply still
    // listening. So the two phases are selected on separately: `server` alone
    // is the normal state, and only once the shutdown signal arrives does the
    // clock start on how long an open connection (an SSE stream, say) may hold
    // the drain open before we give up on it.
    let signal = workers.signal();
    // `with_graceful_shutdown` yields an `IntoFuture`, not a `Future`, so it
    // has to be converted before it can be pinned and polled in a `select!`.
    let server = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            info!("shutdown signal received, graceful shutdown initiated");
            signal.stop();
        })
        .into_future();
    tokio::pin!(server);

    let drain_deadline = async {
        shutdown_signal().await;
        tokio::time::sleep(SHUTDOWN_DEADLINE).await;
    };

    tokio::select! {
        // `biased` polls the server first, so a signal that has already been
        // handled by the shutdown future above is never mistaken for a timeout.
        biased;
        result = &mut server => {
            if let Err(error) = result {
                error!(%error, "HTTP server stopped with an error");
            }
        }
        _ = drain_deadline => error!(
            "HTTP server did not drain within {}s; abandoning open connections",
            SHUTDOWN_DEADLINE.as_secs()
        ),
    }

    if tokio::time::timeout(WORKER_STOP_DEADLINE, workers.stop())
        .await
        .is_err()
    {
        error!(
            "background workers did not stop within {}s; exiting anyway",
            WORKER_STOP_DEADLINE.as_secs()
        );
    }

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
