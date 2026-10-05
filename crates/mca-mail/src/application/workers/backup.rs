//! Background backup worker: starts the supervisor and journals its outcomes.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::backup::{BackupEvent, BackupService, BackupSupervisor};
use crate::config::BackupSettings;

mod journal;

/// Code recorded in `processing_events.code` for a completed backup.
pub const EVENT_BACKUP_COMPLETED: &str = "backup.completed";
/// Code recorded for a failed scheduled backup.
pub const EVENT_BACKUP_FAILED: &str = "backup.failed";

/// Start the periodic backup loop and a journal task that records every
/// outcome. Returns `None` when `BACKUP_ENABLED` is false, so a disabled
/// deployment spawns nothing at all.
pub fn spawn(
    settings: &BackupSettings,
    database_url: &str,
    pool: PgPool,
    shutdown: watch::Receiver<bool>,
) -> Option<JoinHandle<()>> {
    if !settings.enabled {
        tracing::info!("automatic backups disabled by BACKUP_ENABLED");
        return None;
    }
    let service = match BackupService::new(settings.clone(), database_url) {
        Ok(service) => Arc::new(service),
        Err(e) => {
            // Backups must never take the server down, but staying silent would
            // hide a broken DATABASE_URL until someone needs a restore.
            tracing::error!(error = %e, "automatic backups cannot start");
            return None;
        }
    };
    let supervisor = BackupSupervisor::new(service);
    let (events, mut journal) = mpsc::channel::<BackupEvent>(16);

    // Move the pool out to the journal task, then hand the receiver back so the
    // loop task can close the channel when the supervisor returns.
    let journal_pool = pool.clone();
    let journal_task = tokio::spawn(async move {
        while let Some(event) = journal.recv().await {
            if let Err(e) = journal::record(&journal_pool, event).await {
                // Losing the journal entry must not stop the backup loop.
                tracing::error!(error = %e, "could not journal backup event");
            }
        }
    });

    let loop_task = tokio::spawn(async move {
        supervisor.run(shutdown, events).await;
        // The sender lives only inside the supervisor; closing it here lets the
        // journal task see `None` and finish.
        let _ = journal_task.await;
    });

    tracing::info!(
        dir = %settings.dir.display(),
        interval_hours = settings.interval_hours,
        retention_days = settings.retention_days,
        retention_weeks = settings.retention_weeks,
        "backup worker started"
    );
    Some(loop_task)
}

#[cfg(test)]
#[path = "backup/tests.rs"]
mod tests;
