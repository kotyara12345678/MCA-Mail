//! MCA Mail entry point.

use std::net::SocketAddr;

use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing/logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .json()
        .init();

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

    // Start background workers (mail poll, retention).
    mca_mail::application::workers::spawn_all(
        &app.config,
        app.pool.clone(),
        app.orchestrator.clone(),
    )
    .await;

    // Build and start the HTTP server
    let router = mca_mail::api::build_router(&app.config.api, app.state.clone());

    let addr: SocketAddr = format!("{}:{}", app.config.api.host, app.config.api.port)
        .parse()
        .expect("valid socket address");

    info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

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

    info!("shutdown signal received, graceful shutdown initiated");
}
