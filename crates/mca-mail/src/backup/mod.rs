//! The backup subsystem: dump, validate, rotate.
//!
//! Independent of `MAIL_MODE` on purpose. Read-only protects the customer's
//! mailbox; it says nothing about the agent's own records, which are the only
//! thing that survives a lost server.

mod directory;
mod dump;
mod error;
mod name;
#[cfg(test)]
#[path = "name_test.rs"]
mod name_test;
mod pgpass;
mod process;
mod retention;
#[cfg(test)]
#[path = "retention_boundary_test.rs"]
mod retention_boundary_test;
#[cfg(test)]
#[path = "retention_test.rs"]
mod retention_test;
mod rotation;
mod runner;
mod service;
mod supervisor;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use std::path::Path;

pub use error::BackupError;
pub use name::{BackupName, EXTENSION, PREFIX, TEMP_SUFFIX};
pub use retention::RetentionPolicy;
pub use runner::connection_arguments;
pub use service::{newest_backup, BackupOutcome, BackupService};
pub use supervisor::{BackupEvent, BackupSupervisor};

pub(crate) fn prepare_directory(dir: &Path) -> Result<(), BackupError> {
    directory::ensure_dir(dir)
}
