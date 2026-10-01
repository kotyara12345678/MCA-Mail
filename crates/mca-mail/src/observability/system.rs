//! System events: migrations, database, transactions, restart recovery.

use tracing::{error, info};

use super::{errors::clip, EVENT_TARGET};

/// Database migrations started (component `database`).
pub fn migration_started() {
    info!(target: EVENT_TARGET, component = "database", "migration_started");
}

/// Database migrations finished successfully.
pub fn migration_completed(duration_ms: u64) {
    info!(
        target: EVENT_TARGET,
        component = "database",
        duration_ms,
        "migration_completed"
    );
}

/// The initial database connection could not be established.
pub fn database_connection_failed(error_type: &str, error: &str) {
    error!(
        target: EVENT_TARGET,
        component = "database",
        operation = "connect",
        error_type,
        error = clip(error, 300),
        "database_connection_failed"
    );
}

/// A repository (sqlx) call failed.
pub fn repository_error(operation: &str, error_type: &str, error: &str) {
    error!(
        target: EVENT_TARGET,
        component = "repository",
        operation,
        error_type,
        error = clip(error, 300),
        "repository_error"
    );
}

/// A database transaction failed and was rolled back.
pub fn transaction_failed(operation: &str, error_type: &str, error: &str) {
    error!(
        target: EVENT_TARGET,
        component = "transaction",
        operation,
        error_type,
        error = clip(error, 300),
        "transaction_failed"
    );
}

/// Startup recovery of runs interrupted by a crash/restart began.
pub fn recovery_started() {
    info!(target: EVENT_TARGET, "recovery_started");
}

/// How many rows looked interrupted before recovery touched them.
pub fn stuck_emails_found(runs: i64, emails: i64) {
    info!(target: EVENT_TARGET, runs, emails, "stuck_emails_found");
}

/// One interrupted email was released back to `pending`.
pub fn processing_recovered(email_id: crate::domain::EmailId) {
    info!(
        target: EVENT_TARGET,
        email_id = %email_id,
        "processing_recovered"
    );
}

/// Startup recovery finished.
pub fn recovery_completed(runs: i64, emails: i64, duration_ms: u64) {
    info!(
        target: EVENT_TARGET,
        runs,
        emails,
        duration_ms,
        "recovery_completed"
    );
}
