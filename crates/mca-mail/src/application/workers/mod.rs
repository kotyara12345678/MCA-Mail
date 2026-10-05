//! Background workers: mail reading, queue processing, backups, retention.

pub mod backup;
mod cursor;
pub mod fallback;
pub mod idle;
mod outbox;
pub mod poll;
pub mod queue;
pub mod recovery;
pub mod spawn;
mod spawn_mail;
pub mod sync;

pub use outbox::outbox_loop;

use std::sync::Arc;

use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{error, info};

use crate::config::RetentionSettings;
use crate::shutdown::{stopping, Rx};

pub use spawn::{spawn_all, WorkerSet};
pub use sync::{sync_channel, MailSync, SyncTrigger};

/// Periodic retention job (email bodies anonymization, event purge).
///
/// Runs on the hour scale, so it waits for the ticker rather than polling; the
/// shutdown check sits in the same `select!` so a retention pass is never
/// started after the process has been asked to go down.
pub async fn retention_loop(pool: PgPool, settings: RetentionSettings, mut shutdown: Rx) {
    let mut ticker = interval(Duration::from_secs(
        (settings.check_interval_hours.max(1) as u64) * 3600,
    ));
    info!("retention loop started");

    loop {
        tokio::select! {
            _ = stopping(&mut shutdown) => break,
            _ = ticker.tick() => {}
        }
        match crate::persistence::retention_repo::run(&pool, &settings).await {
            Ok(report) => {
                info!(report = ?report, "retention cycle completed");
            }
            Err(e) => {
                error!(error = %e, "retention cycle failed");
            }
        }
    }
    info!("retention loop stopped");
}
