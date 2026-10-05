use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use super::{directory, dump, rotation, BackupError, RetentionPolicy};
use crate::config::BackupSettings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupOutcome {
    pub file_name: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub removed: Vec<String>,
}

pub struct BackupService {
    settings: BackupSettings,
    url: url::Url,
    running: Arc<Mutex<bool>>,
}

impl BackupService {
    pub fn new(settings: BackupSettings, database_url: &str) -> Result<Self, BackupError> {
        let url = url::Url::parse(database_url)
            .map_err(|e| BackupError::Config(format!("DATABASE_URL is not a valid URI: {e}")))?;
        Ok(Self {
            settings,
            url,
            running: Arc::new(Mutex::new(false)),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.settings.enabled
    }

    pub fn dir(&self) -> &Path {
        &self.settings.dir
    }

    pub fn settings_snapshot(&self) -> &BackupSettings {
        &self.settings
    }

    pub fn from_config(config: &crate::config::AppConfig) -> Result<Self, BackupError> {
        Self::new(config.backup.clone(), &config.database.url)
    }

    pub async fn run_once(&self) -> Result<BackupOutcome, BackupError> {
        if !self.settings.enabled {
            return Err(BackupError::Config("BACKUP_ENABLED is false".into()));
        }
        {
            let mut running = self.running.lock().await;
            if *running {
                return Err(BackupError::AlreadyRunning);
            }
            *running = true;
        }
        let result = self.run_locked().await;
        *self.running.lock().await = false;
        result
    }

    async fn run_locked(&self) -> Result<BackupOutcome, BackupError> {
        let settings = self.settings.clone();
        let url = self.url.clone();
        let outcome =
            tokio::task::spawn_blocking(move || dump::dump_once(&settings, &url)).await??;
        let removed = self.rotate().await?;
        Ok(BackupOutcome { removed, ..outcome })
    }

    pub async fn rotate(&self) -> Result<Vec<String>, BackupError> {
        let dir = self.settings.dir.clone();
        let policy =
            RetentionPolicy::new(self.settings.retention_days, self.settings.retention_weeks);
        let removed =
            tokio::task::spawn_blocking(move || rotation::rotate_once(&dir, policy)).await??;
        for name in &removed {
            tracing::info!(file = %name, "rotated out old backup");
        }
        Ok(removed)
    }
}

pub fn newest_backup(dir: &Path) -> Result<Option<PathBuf>, BackupError> {
    Ok(directory::scan(dir)?
        .newest()
        .map(|backup| dir.join(backup.file_name())))
}
