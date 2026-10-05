//! Periodic backup loop.
//!
//! Backups run regardless of `MAIL_MODE`: read-only stops the agent touching a
//! customer's mailbox, but the agent's own leads, drafts and history are still
//! the only record of what happened and would be lost with the disk.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::backup::{BackupOutcome, BackupService};
use crate::error::AppError;

pub struct BackupSupervisor {
    service: Arc<BackupService>,
}

impl BackupSupervisor {
    pub fn new(service: Arc<BackupService>) -> Self {
        Self { service }
    }

    /// Run until cancelled, emitting one event per completed or failed attempt.
    ///
    /// The interval is re-read on every tick, so lowering `BACKUP_INTERVAL_HOURS`
    /// takes effect without a restart once the current sleep ends.
    pub async fn run(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
        events: mpsc::Sender<BackupEvent>,
    ) {
        if !self.service.is_enabled() {
            tracing::info!("automatic backups disabled by BACKUP_ENABLED");
            return;
        }

        if self.service.settings_snapshot().run_on_startup {
            self.attempt(&events).await;
        }

        loop {
            let wait = self.service.settings_snapshot().interval();
            tokio::select! {
                _ = tokio::time::sleep(wait) => self.attempt(&events).await,
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!("backup supervisor stopping");
                        return;
                    }
                }
            }
        }
    }

    async fn attempt(&self, events: &mpsc::Sender<BackupEvent>) {
        match self.service.run_once().await {
            Ok(outcome) => {
                tracing::info!(
                    file = %outcome.file_name,
                    size_bytes = outcome.size_bytes,
                    rotated_out = outcome.removed.len(),
                    "scheduled backup finished"
                );
                let _ = events.send(BackupEvent::Completed(outcome)).await;
            }
            Err(error) => {
                // A failed backup is an operational event, not a crash: the loop
                // continues so the next interval can recover on its own.
                tracing::error!(%error, "scheduled backup failed");
                let _ = events.send(BackupEvent::Failed(error.to_string())).await;
            }
        }
    }
}

/// What the supervisor reports to the event journal and the health endpoint.
#[derive(Debug, Clone)]
pub enum BackupEvent {
    Completed(BackupOutcome),
    Failed(String),
}

impl BackupSupervisor {
    /// One-shot backup for the CLI and for tests. Ignores the schedule.
    pub async fn run_now(&self) -> Result<BackupOutcome, AppError> {
        self.service
            .run_once()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))
    }
}
