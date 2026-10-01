//! Queue and poll cycle events: fetch, claim, workers.

use tracing::{error, info};

use super::{errors::clip, EVENT_TARGET};

/// A mailbox poll cycle started.
pub fn poll_started(mailbox: &str) {
    info!(target: EVENT_TARGET, mailbox, "poll_started");
}

/// A mailbox poll cycle finished (fetch + store).
pub fn poll_completed(mailbox: &str, duration_ms: u64, fetched: i64) {
    info!(
        target: EVENT_TARGET,
        mailbox,
        duration_ms,
        fetched,
        "poll_completed"
    );
}

/// How many new messages the provider returned this cycle.
pub fn emails_fetched(count: i64) {
    info!(target: EVENT_TARGET, count, "emails_fetched");
}

/// How many messages were stored this cycle.
pub fn emails_inserted(count: i64) {
    info!(target: EVENT_TARGET, count, "emails_inserted");
}

/// How many messages were skipped as duplicates this cycle.
pub fn emails_skipped_duplicate(count: i64) {
    info!(target: EVENT_TARGET, count, "emails_skipped_duplicate");
}

/// The queue claimed a batch of pending emails for processing.
pub fn batch_claimed(size: i64) {
    info!(target: EVENT_TARGET, size, "batch_claimed");
}

/// The queue finished a batch: how many succeeded or failed.
pub fn batch_completed(size: i64, ok: i64, failed: i64, duration_ms: u64) {
    info!(
        target: EVENT_TARGET,
        size,
        ok,
        failed,
        duration_ms,
        "batch_completed"
    );
}

/// A worker task started processing one claimed email.
pub fn worker_started(email_id: crate::domain::EmailId) {
    info!(target: EVENT_TARGET, email_id = %email_id, "worker_started");
}

/// A worker task finished successfully for one email.
pub fn worker_completed(email_id: crate::domain::EmailId, duration_ms: u64) {
    info!(
        target: EVENT_TARGET,
        email_id = %email_id,
        duration_ms,
        "worker_completed"
    );
}

/// A worker task failed for one email.
pub fn worker_failed(email_id: crate::domain::EmailId, error: &str, duration_ms: u64) {
    error!(
        target: EVENT_TARGET,
        email_id = %email_id,
        error = clip(error, 300),
        duration_ms,
        "worker_failed"
    );
}
