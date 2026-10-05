//! Backup-specific startup validation.
//!
//! Runs before the service starts so a bad retention window or a directory that
//! cannot be created is a startup error rather than a surprise at 3am.

use crate::config::BackupSettings;
use crate::error::ConfigError;

pub fn check(settings: &BackupSettings) -> Result<(), ConfigError> {
    if !settings.enabled {
        // Nothing else matters when backups are off, and a disabled backup must
        // not fail startup over a directory it will never touch.
        return Ok(());
    }

    if settings.interval_hours == 0 {
        return Err(field("BACKUP_INTERVAL_HOURS must be at least 1"));
    }
    if settings.interval_hours > 24 * 365 {
        return Err(field("BACKUP_INTERVAL_HOURS must be at most 8760"));
    }
    if settings.retention_days == 0 {
        return Err(field(
            "BACKUP_RETENTION_DAYS must be at least 1, otherwise no backup would survive",
        ));
    }
    if settings.max_size_mb == 0 {
        return Err(field("BACKUP_MAX_SIZE_MB must be at least 1"));
    }
    if !(10..=86_400).contains(&settings.command_timeout_seconds) {
        return Err(field(
            "BACKUP_COMMAND_TIMEOUT_SECONDS must be between 10 and 86400",
        ));
    }
    if settings.dir.as_os_str().is_empty() {
        return Err(field("BACKUP_DIR must not be empty"));
    }
    // A relative path would resolve against whatever the working directory
    // happens to be under systemd, which is `/` — the backups would vanish.
    if settings.dir.is_relative() {
        return Err(field("BACKUP_DIR must be an absolute path"));
    }
    if settings.retention_weeks > 52 {
        return Err(field("BACKUP_RETENTION_WEEKS must be at most 52"));
    }
    crate::backup::prepare_directory(&settings.dir).map_err(|error| ConfigError::Invalid {
        field: "BACKUP_DIR".into(),
        reason: error.to_string(),
    })?;
    Ok(())
}

fn field(message: &'static str) -> ConfigError {
    ConfigError::Invalid {
        field: "backup".into(),
        reason: message.to_string(),
    }
}

#[cfg(test)]
#[path = "backup_tests.rs"]
mod tests;
