//! PostgreSQL backup settings.
//!
//! Backups are deliberately independent of [`MailMode`]: read-only protects a
//! customer's mailbox, not the agent's own data, and the agent's records are the
//! only thing that survives a lost server.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupSettings {
    pub enabled: bool,
    pub dir: PathBuf,
    /// How often a dump is taken. Minimum 1 hour.
    pub interval_hours: u64,
    /// Daily snapshots kept. Minimum 1, so there is always something to restore.
    pub retention_days: u32,
    /// Weekly snapshots kept, chosen from the dailies.
    pub retention_weeks: u32,
    /// Refuse to write a dump larger than this. Guards against a runaway table.
    pub max_size_mb: u64,
    /// `pg_dump` / `pg_restore` timeout per run.
    pub command_timeout_seconds: u64,
    /// Run one dump at startup instead of waiting a full interval.
    pub run_on_startup: bool,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: PathBuf::from("/app/backups"),
            interval_hours: 6,
            retention_days: 7,
            retention_weeks: 4,
            max_size_mb: 1024,
            command_timeout_seconds: 600,
            run_on_startup: true,
        }
    }
}

impl BackupSettings {
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_hours.max(1) * 3600)
    }

    pub fn command_timeout(&self) -> Duration {
        Duration::from_secs(self.command_timeout_seconds.max(10))
    }

    pub fn max_size_bytes(&self) -> u64 {
        self.max_size_mb.max(1).saturating_mul(1024 * 1024)
    }
}
