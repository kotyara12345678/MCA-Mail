//! IMAP IDLE watcher events.
//!
//! These are the signals an operator needs to tell "the watcher is working"
//! from "the watcher stopped years ago": one heartbeat per completed cycle,
//! one line per notification, and one per failure that starts a backoff.

use tracing::{debug, error, info};

use super::{errors::clip, EVENT_TARGET};

/// A cycle ended on the RFC's own re-issue deadline.
///
/// Doubles as the heartbeat: seeing it every `MAIL_IDLE_WAIT_SECONDS` means the
/// connection is alive and the server is still talking to us.
pub fn reissued(seconds: u64) {
    debug!(target: EVENT_TARGET, seconds, "idle_reissued");
}

/// The server reported a mailbox change; a read was requested.
pub fn new_mail() {
    info!(target: EVENT_TARGET, "idle_new_mail");
}

/// A connection was lost and a backoff is starting.
pub fn retrying(error: &str, delay_seconds: u64, attempt: u32) {
    error!(
        target: EVENT_TARGET,
        error = clip(error, 300),
        delay_seconds,
        attempt,
        "idle_retry"
    );
}

/// The watcher will not come back: this server or account cannot idle.
pub fn unsupported(error: &str) {
    error!(
        target: EVENT_TARGET,
        error = clip(error, 300),
        "idle_unsupported"
    );
}
