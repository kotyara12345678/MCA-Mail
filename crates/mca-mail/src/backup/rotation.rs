//! Rotation: delete the backups the policy no longer keeps.
//!
//! Everything here is deliberately conservative. The failure that matters is
//! deleting a backup that was the only one, so the newest is never a candidate
//! and every deletion re-checks the entry type at the moment of removal.

use std::path::Path;

use chrono::Utc;

use super::directory;
use super::error::BackupError;
use super::name::BackupName;
use super::retention::RetentionPolicy;

/// A temp file older than this cannot belong to a live run: it is at most as old
/// as the configured command timeout, and a day is longer than any sane timeout.
const STALE_TEMP_AGE_SECS: u64 = 2 * 24 * 3600;

/// Apply the policy to the directory. Returns the file names removed.
pub fn rotate_once(dir: &Path, policy: RetentionPolicy) -> Result<Vec<String>, BackupError> {
    let listing = directory::scan(dir)?;

    for stale in &listing.stale_temp_files {
        if is_older_than(stale, STALE_TEMP_AGE_SECS) && std::fs::remove_file(stale).is_ok() {
            tracing::warn!(file = %stale.display(), "removed stale partial backup");
        }
    }

    let mut removed = Vec::new();
    for backup in policy.select(&listing.backups, Utc::now()) {
        let path = dir.join(backup.file_name());
        // Re-check the entry type at deletion time: between the scan and here,
        // the name could have been replaced with a symlink.
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_symlink()) {
            tracing::warn!(file = %path.display(), "refusing to delete a symlink");
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            removed.push(backup.file_name());
        }
    }
    Ok(removed)
}

fn is_older_than(path: &Path, secs: u64) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() >= secs)
}

/// The names rotation would remove, without touching the filesystem.
pub fn doomed(dir: &Path, policy: RetentionPolicy) -> Result<Vec<BackupName>, BackupError> {
    Ok(policy.select(&directory::scan(dir)?.backups, Utc::now()))
}
