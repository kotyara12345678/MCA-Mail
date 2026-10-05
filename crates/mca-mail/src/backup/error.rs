//! Failures of the backup subsystem.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("backup configuration error: {0}")]
    Config(String),
    #[error("backup filesystem error: {0}")]
    Io(String),
    /// `pg_dump` or `pg_restore` could not be started at all.
    #[error("backup tool unavailable: {0}")]
    ToolMissing(String),
    /// The tool ran and exited non-zero. Carries the exit code and stderr.
    #[error("backup tool failed with status {code}: {stderr}")]
    ToolFailed { code: i32, stderr: String },
    /// The tool was killed or ran past the configured timeout.
    #[error("backup timed out after {0}s")]
    Timeout(u64),
    #[error("backup produced no usable file: {0}")]
    Empty(String),
    #[error("backup exceeded the configured size limit: {0}")]
    TooLarge(String),
    /// A dump exists but does not pass `pg_restore --list`.
    #[error("backup failed validation: {0}")]
    Invalid(String),
    /// A second run tried to start while one was in flight.
    #[error("a backup is already running")]
    AlreadyRunning,
    /// The blocking task running `pg_dump` panicked or was cancelled.
    #[error("backup task did not finish: {0}")]
    Task(String),
}

impl From<tokio::task::JoinError> for BackupError {
    fn from(error: tokio::task::JoinError) -> Self {
        BackupError::Task(error.to_string())
    }
}
