//! Email lifecycle events: receive, processing start/finish/fail.

use tracing::{error, info};

use super::{Correlation, EVENT_TARGET};

/// A new inbound message was stored (poll cycle).
pub fn email_received(
    email_id: crate::domain::EmailId,
    thread_id: crate::domain::ThreadId,
    message_id: Option<&str>,
) {
    match message_id {
        Some(mid) => info!(
            target: EVENT_TARGET,
            email_id = %email_id,
            thread_id = %thread_id,
            message_id = mid,
            "email_received"
        ),
        None => info!(
            target: EVENT_TARGET,
            email_id = %email_id,
            thread_id = %thread_id,
            "email_received"
        ),
    }
}

/// The pipeline claimed the email and started a run.
pub fn processing_started(c: &Correlation) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        "processing_started"
    );
}

/// The pipeline finished with a final status.
pub fn processing_completed(c: &Correlation, status: &str, duration_ms: u64) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        status,
        duration_ms,
        "processing_completed"
    );
}

/// The pipeline failed; `retry_count` is how often it already tried.
pub fn processing_failed(c: &Correlation, error_type: &str, error: &str, retry_count: i32) {
    error!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        error_type,
        error = super::errors::clip(error, 300),
        retry_count,
        "processing_failed"
    );
}
