//! Inspecting the backup directory, safely.
//!
//! Two rules govern every read here: only names that parse as backups are
//! returned, and symlinks are never followed or deleted. Together they mean a
//! crafted file name in `BACKUP_DIR` cannot make rotation remove something
//! outside it.

use std::path::{Path, PathBuf};

use super::error::BackupError;
use super::name::BackupName;

#[derive(Debug, Default)]
pub struct DirectoryListing {
    /// Finished backups, parsed and sorted oldest first.
    pub backups: Vec<BackupName>,
    /// `*.partial` leftovers from a crashed run.
    pub stale_temp_files: Vec<PathBuf>,
    /// Entries that are not backups and are never touched.
    pub foreign: Vec<PathBuf>,
}

impl DirectoryListing {
    pub fn newest(&self) -> Option<&BackupName> {
        self.backups.last()
    }
    pub fn len(&self) -> usize {
        self.backups.len()
    }
    pub fn is_empty(&self) -> bool {
        self.backups.is_empty()
    }
}

/// Read `dir` without following symlinks.
pub fn scan(dir: &Path) -> Result<DirectoryListing, BackupError> {
    match std::fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(BackupError::Io(
                "BACKUP_DIR must not be a symbolic link".into(),
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(BackupError::Io("BACKUP_DIR is not a directory".into()));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DirectoryListing::default());
        }
        Err(error) => {
            return Err(BackupError::Io(format!("stat {}: {error}", dir.display())));
        }
        _ => {}
    }
    let mut listing = DirectoryListing::default();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| BackupError::Io(format!("read_dir {}: {e}", dir.display())))?;

    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| BackupError::Io(format!("stat {}: {e}", path.display())))?;
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if file_type.is_symlink() {
            // Never resolved, never deleted: a link inside the backup directory
            // could point anywhere, including at a user file.
            tracing::warn!(path = %path.display(), "ignoring symlink in backup directory");
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if let Some(backup) = BackupName::parse(name) {
            listing.backups.push(backup);
        } else if BackupName::is_temp(name) {
            listing.stale_temp_files.push(path);
        } else if name != ".pgpass" {
            listing.foreign.push(path);
        }
    }

    listing.backups.sort_by_key(|b| b.created_at);
    listing.stale_temp_files.sort();
    Ok(listing)
}

/// Create the directory with owner-only permissions if it does not exist.
///
/// Backups contain personal and commercial data, so the directory itself is the
/// first access control: 0700 on Unix.
pub fn ensure_dir(dir: &Path) -> Result<(), BackupError> {
    match std::fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(BackupError::Io(
                "BACKUP_DIR must not be a symbolic link".into(),
            ));
        }
        Ok(metadata) if metadata.is_dir() => return tighten(dir),
        Ok(_) => return Err(BackupError::Io("BACKUP_DIR is not a directory".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(BackupError::Io(format!("stat {}: {error}", dir.display()))),
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| BackupError::Io(format!("create {}: {e}", dir.display())))?;
    tighten(dir)
}

#[cfg(unix)]
fn tighten(dir: &Path) -> Result<(), BackupError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| BackupError::Io(format!("chmod {}: {e}", dir.display())))
}

#[cfg(not(unix))]
fn tighten(_dir: &Path) -> Result<(), BackupError> {
    // On Windows the directory inherits its ACL from the parent; the deployment
    // documentation requires restricting `BACKUP_DIR` via NTFS permissions.
    Ok(())
}
