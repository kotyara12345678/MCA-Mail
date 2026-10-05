//! The single reader of the mailbox.
//!
//! Everything else in the IDLE design exists only to shorten the wait between
//! reads: the watcher reports "there is mail now", the fallback poller reports
//! "it has been a while, look anyway", and both do it by sending the same
//! one-slot trigger. Reading happens here and nowhere else, so a burst of
//! notifications can never turn into a burst of concurrent `FETCH`es.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::{error, info};

use crate::mail::MailProvider;
use crate::shutdown::{stopping, Rx};

use super::poll::poll_once;

/// One-slot channel: capacity one, `try_send`, never blocking.
///
/// Coalescing is the point. Five `EXISTS` notifications arriving in the same
/// second must cause one fetch, and a notifier must never be blocked — or made
/// to wait — behind a fetch that is still running.
#[derive(Clone)]
pub struct MailSync {
    tx: mpsc::Sender<()>,
}

/// The receiving end, owned by [`sync_loop`].
pub type SyncTrigger = mpsc::Receiver<()>;

/// Create the channel the notifiers and the reader share.
pub fn sync_channel() -> (MailSync, SyncTrigger) {
    let (tx, rx) = mpsc::channel(1);
    (MailSync { tx }, rx)
}

impl MailSync {
    /// Ask for a read. `false` means one is already queued, which is a success
    /// condition rather than a failure: the pending read will see the new mail.
    pub fn notify(&self) -> bool {
        self.tx.try_send(()).is_ok()
    }
}

/// Read the mailbox when the interval says so, or when a notifier says so.
pub async fn sync_loop(
    provider: Arc<dyn MailProvider>,
    pool: PgPool,
    mailbox: String,
    poll_seconds: u64,
    mut trigger: SyncTrigger,
    mut shutdown: Rx,
) {
    let mut ticker = interval(Duration::from_secs(poll_seconds.max(5)));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut notifiers_alive = true;
    info!(mailbox = %mailbox, poll_seconds, "mail sync loop started");

    loop {
        let woken = tokio::select! {
            _ = stopping(&mut shutdown) => break,
            _ = ticker.tick() => true,
            notified = trigger.recv(), if notifiers_alive => match notified {
                // Every notifier has exited. The interval still runs, so mail
                // keeps moving rather than stopping because IDLE went away.
                None => { notifiers_alive = false; false }
                Some(()) => true,
            },
        };
        if !woken {
            continue;
        }
        if let Err(e) = poll_once(provider.as_ref(), &pool, &mailbox).await {
            error!(error = %e, "mail sync cycle failed");
        }
    }
    info!("mail sync loop stopped");
}
