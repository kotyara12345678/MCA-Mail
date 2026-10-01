//! Background workers: periodic mail polling, processing, recovery, retention.

pub mod poll;
pub mod queue;
pub mod recovery;

use std::sync::Arc;

use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{error, info};

use crate::config::AppConfig;
use crate::mail::MailProvider;
use crate::orchestration::Orchestrator;

pub use poll::mail_poll_loop;
pub use queue::queue_loop;
pub use recovery::recover_stuck_emails;

/// Periodic retention job (email bodies anonymization, event purge).
pub async fn retention_loop(pool: PgPool, settings: crate::config::RetentionSettings) {
    let mut ticker = interval(Duration::from_secs(
        (settings.check_interval_hours.max(1) as u64) * 3600,
    ));
    info!("retention loop started");

    loop {
        ticker.tick().await;
        match crate::persistence::retention_repo::run(&pool, &settings).await {
            Ok(report) => {
                info!(report = ?report, "retention cycle completed");
            }
            Err(e) => {
                error!(error = %e, "retention cycle failed");
            }
        }
    }
}

/// Start all background loops.
pub async fn spawn_all(config: &AppConfig, pool: PgPool, orchestrator: Arc<Orchestrator>) {
    let provider = match crate::mail::build(&config.mail) {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "failed to build mail provider; poll loop disabled");
            return;
        }
    };
    let provider: Arc<dyn MailProvider> = provider.into();

    if let Err(e) = provider.init().await {
        error!(error = %e, "failed to initialise mail provider; poll loop disabled");
        return;
    }
    info!(provider = %provider.name(), "mail provider initialised");

    if let Err(e) = recover_stuck_emails(&pool).await {
        error!(error = %e, "failed to recover interrupted emails");
    }

    let mailbox = config.mail.inbox().to_string();
    let poll_seconds = config.mail.poll_interval_seconds;

    tokio::spawn(mail_poll_loop(
        provider,
        pool.clone(),
        orchestrator.clone(),
        poll_seconds,
        mailbox,
    ));

    tokio::spawn(queue_loop(pool.clone(), orchestrator));

    if config.retention.enabled {
        tokio::spawn(retention_loop(pool, config.retention.clone()));
    }
}
