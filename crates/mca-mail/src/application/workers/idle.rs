//! The IDLE worker: one waiter, one backoff, one clean exit.

use std::sync::Arc;
use std::time::Instant;

use tracing::{error, info};

use crate::mail::idle::{Backoff, IdleError, IdleEvent, IdleSource};
use crate::observability::idle as events;
use crate::shutdown::{pause, Rx};

use super::sync::MailSync;

#[cfg(test)]
#[path = "idle_tests.rs"]
mod tests;

/// Follow the mailbox until told to stop.
///
/// The backoff lives here rather than inside the source: a source that retried
/// on its own could not be driven by a script in tests, and could not be asked
/// to stop while it was sleeping.
pub async fn idle_loop(
    source: Arc<dyn IdleSource>,
    sync: MailSync,
    backoff: Backoff,
    mut shutdown: Rx,
) {
    let mut backoff = backoff;
    info!("IMAP IDLE watcher started");

    loop {
        // Opening a connection only to notice the flag and close it again is a
        // round trip nobody asked for. The flag is sticky, so one check at the
        // top of each pass is enough.
        if *shutdown.borrow() {
            break;
        }
        let cycle_started = Instant::now();
        match source.wait(&mut shutdown).await {
            Ok(IdleEvent::Stopped) => break,
            Ok(IdleEvent::NewMail) => {
                // A full cycle completed, so the connection is healthy again.
                backoff.reset();
                events::new_mail();
                sync.notify();
            }
            Ok(IdleEvent::Timeout) => {
                backoff.reset();
                events::reissued(cycle_started.elapsed().as_secs());
            }
            Err(IdleError::Unsupported(reason)) => {
                error!(error = %reason, "IMAP IDLE unavailable; watcher stopped");
                events::unsupported(&reason.to_string());
                break;
            }
            Err(IdleError::Transient(reason)) => {
                let delay = backoff.next_delay();
                events::retrying(&reason.to_string(), delay.as_secs(), backoff.attempt());
                if pause(&mut shutdown, delay).await {
                    break;
                }
            }
        }
    }
    // Always, including the paths that break without a session: closing an
    // already-empty source is a no-op, and skipping it would leak a socket.
    source.close().await;
    info!("IMAP IDLE watcher stopped");
}
