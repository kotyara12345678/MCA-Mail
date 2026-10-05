//! Cooperative shutdown shared by the HTTP server and every background loop.
//!
//! One flag, raised once: each worker finishes the step it is on, releases what
//! it holds — an open IMAP session in particular — and returns. Every helper
//! here can be interrupted by the same signal, so no worker has to choose
//! between sleeping out a backoff and noticing that the process is going down.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

/// The receiving end a worker holds. Cheap to clone; each clone tracks its own
/// "seen" version, which is harmless because the flag only ever goes `false`
/// to `true`.
pub type Rx = watch::Receiver<bool>;

/// The sending end, kept by whoever owns the process lifetime.
#[derive(Clone)]
pub struct Signal {
    tx: Arc<watch::Sender<bool>>,
}

impl Signal {
    /// A signal that starts unstopped, plus the first receiver for it.
    pub fn new() -> (Self, Rx) {
        let (tx, rx) = watch::channel(false);
        (Self { tx: Arc::new(tx) }, rx)
    }

    /// Request that every worker stop. A receiver that already went away means
    /// the process is going down anyway, so there is nothing to report.
    pub fn stop(&self) {
        let _ = self.tx.send(true);
    }

    /// A fresh receiver on the same flag, for code that wants `watch` directly
    /// rather than [`Rx`] — the backup supervisor, for instance.
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.tx.subscribe()
    }
}

impl Default for Signal {
    fn default() -> Self {
        Self::new().0
    }
}

/// Resolves once a stop has been requested, or the sender disappears.
pub async fn stopping(rx: &mut Rx) -> bool {
    loop {
        if *rx.borrow_and_update() {
            return true;
        }
        if rx.changed().await.is_err() {
            return true;
        }
    }
}

/// Wait for `delay`, returning early — and reporting it — if a stop arrives.
///
/// Returns `true` when the wait was cut short by a stop.
pub async fn pause(rx: &mut Rx, delay: Duration) -> bool {
    if *rx.borrow_and_update() {
        return true;
    }
    tokio::select! {
        _ = tokio::time::sleep(delay) => false,
        _ = rx.changed() => true,
    }
}

#[cfg(test)]
#[path = "shutdown_tests.rs"]
mod tests;
