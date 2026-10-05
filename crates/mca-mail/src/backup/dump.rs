//! One dump: write to a temp file, validate it, then publish it atomically.

use std::path::Path;

use chrono::Utc;

use super::directory;
use super::error::BackupError;
use super::name::BackupName;
use super::runner;
use super::BackupOutcome;
use crate::config::BackupSettings;

/// Take one dump. Blocking by design: it is called from `spawn_blocking`.
///
/// Any failure leaves no file under the completed name, so a half-written dump
/// can never be mistaken for a restorable backup.
pub fn dump_once(settings: &BackupSettings, url: &url::Url) -> Result<BackupOutcome, BackupError> {
    directory::ensure_dir(&settings.dir)?;

    let created_at = Utc::now();
    let name = BackupName::new(created_at);
    let temp = settings.dir.join(name.temp_file_name());
    let final_path = settings.dir.join(name.file_name());

    if let Err(error) = build_dump(settings, url, &temp, &final_path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }

    let size_bytes = std::fs::metadata(&final_path)
        .map_err(|e| BackupError::Io(format!("stat {}: {e}", final_path.display())))?
        .len();
    Ok(BackupOutcome {
        file_name: name.file_name(),
        size_bytes,
        created_at,
        removed: Vec::new(),
    })
}

/// The four steps, in the order that makes a failure safe.
fn build_dump(
    settings: &BackupSettings,
    url: &url::Url,
    temp: &Path,
    final_path: &Path,
) -> Result<(), BackupError> {
    let timeout = settings.command_timeout().as_secs();

    // 1. Write to a temp file. Nothing sees a partial file under the final name.
    runner::pg_dump(url, &settings.dir, temp, timeout)?;

    // 2. A zero-length or implausibly large dump is a failure, not a backup.
    let size = file_size(temp)?;
    if size == 0 {
        return Err(BackupError::Empty(format!(
            "pg_dump produced an empty file for {}",
            final_path.display()
        )));
    }
    if size > settings.max_size_bytes() {
        return Err(BackupError::TooLarge(format!(
            "{size} bytes exceeds the {} MB limit",
            settings.max_size_mb
        )));
    }

    // 3. Validate before publishing. A dump that cannot be listed is not a backup.
    runner::pg_restore_list(url, &settings.dir, temp, timeout)?;

    // 4. Publish atomically. A rename within one filesystem cannot be observed
    //    half-finished by a concurrent restore or rotation scan.
    std::fs::rename(temp, final_path)
        .map_err(|e| BackupError::Io(format!("rename into {}: {e}", final_path.display())))?;

    tracing::info!(
        file = %final_path.display(),
        size_bytes = size,
        "backup completed and validated"
    );
    Ok(())
}

fn file_size(path: &Path) -> Result<u64, BackupError> {
    std::fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| BackupError::Io(format!("stat {}: {e}", path.display())))
}
