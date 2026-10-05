//! Backup file naming.
//!
//! Names are the contract between the writer, the rotation logic and the
//! operator doing a manual restore, so parsing is strict: a name that does not
//! match exactly is not a backup, and rotation will never delete it.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

/// `mca-backup-20260105T142233Z-<uuid>.dump`
pub const PREFIX: &str = "mca-backup-";
pub const EXTENSION: &str = ".dump";
/// Suffix used while a dump is still being written.
pub const TEMP_SUFFIX: &str = ".partial";
const STAMP_LEN: usize = 16; // YYYYMMDDTHHMMSSZ
const ID_LEN: usize = 36;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupName {
    pub created_at: DateTime<Utc>,
    id: uuid::Uuid,
}

impl BackupName {
    /// Render the canonical name for a completed backup.
    pub fn new(created_at: DateTime<Utc>) -> Self {
        Self {
            created_at,
            id: uuid::Uuid::new_v4(),
        }
    }

    pub fn file_name(&self) -> String {
        format!(
            "{PREFIX}{}-{id}{EXTENSION}",
            stamp(self.created_at),
            id = self.id
        )
    }

    /// Temporary name for an in-progress dump. The `.partial` suffix is what
    /// makes a crashed run recognisable as incomplete rather than restorable.
    pub fn temp_file_name(&self) -> String {
        format!(
            "{PREFIX}{}-{id}{EXTENSION}{TEMP_SUFFIX}",
            stamp(self.created_at),
            id = self.id
        )
    }

    /// Parse a file name. Returns `None` for anything that is not a backup,
    /// including symlinks and user files that happen to sit in the directory.
    pub fn parse(file_name: &str) -> Option<Self> {
        if !is_backup_name(file_name) {
            return None;
        }
        let name = &file_name[PREFIX.len()..file_name.len() - EXTENSION.len()];
        let (stamp, id) = name.split_once('-')?;
        Some(Self {
            created_at: parse_stamp(stamp)?,
            id: uuid::Uuid::parse_str(id).ok()?,
        })
    }

    /// Whether a directory entry is a finished backup rather than a temp file.
    pub fn is_complete(file_name: &str) -> bool {
        is_backup_name(file_name)
    }

    /// Whether a directory entry is a temp file from a possibly-crashed run.
    pub fn is_temp(file_name: &str) -> bool {
        file_name
            .strip_suffix(TEMP_SUFFIX)
            .is_some_and(|completed| Self::parse(completed).is_some())
    }
}

fn is_backup_name(file_name: &str) -> bool {
    let Some(rest) = file_name.strip_prefix(PREFIX) else {
        return false;
    };
    let Some(stamp) = rest.strip_suffix(EXTENSION) else {
        return false;
    };
    let Some((stamp, id)) = stamp.split_once('-') else {
        return false;
    };
    stamp.len() == STAMP_LEN
        && id.len() == ID_LEN
        && is_digit_stamp(stamp)
        && uuid::Uuid::parse_str(id).is_ok()
}

fn is_digit_stamp(s: &str) -> bool {
    let bytes = s.as_bytes();
    let pattern_ok = bytes[8] == b'T' && bytes[15] == b'Z';
    pattern_ok
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 8 || i == 15 || b.is_ascii_digit())
}

fn stamp(at: DateTime<Utc>) -> String {
    at.format("%Y%m%dT%H%M%SZ").to_string()
}

fn parse_stamp(s: &str) -> Option<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(&s[..8], "%Y%m%d").ok()?;
    let time =
        NaiveDateTime::parse_from_str(&format!("{date}T{}", &s[9..15]), "%Y-%m-%dT%H%M%S").ok()?;
    Some(DateTime::<Utc>::from_naive_utc_and_offset(time, Utc))
}
