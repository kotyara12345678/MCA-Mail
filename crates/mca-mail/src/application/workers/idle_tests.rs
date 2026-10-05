//! Tests for the event-driven side of reading the mailbox.
//!
//! Everything here runs against a scripted waiter and virtual time, so no test
//! opens a socket or waits a real second. What is being pinned down is the
//! contract the socket cannot tell you about: when a notification becomes a
//! read, when a failure becomes a retry, and when either of them stops.

use std::sync::Arc;
use std::time::Duration;

use super::idle_loop;
use crate::application::workers::{fallback, sync::sync_channel};
use crate::mail::idle::scripted::{new_mail, stopped, transient, unsupported, ScriptedIdle};
use crate::mail::idle::{Backoff, IdleSource};
use crate::shutdown::{Rx, Signal};

/// Fast and deterministic: the point of the assertion is that a backoff was
/// waited at all, not how long it was.
fn backoff() -> Backoff {
    Backoff::new(Duration::from_secs(1), Duration::from_secs(1), 0)
}

async fn run(source: Arc<ScriptedIdle>, shutdown: Rx) {
    let (sync, _trigger) = sync_channel();
    idle_loop(source.clone(), sync, backoff(), shutdown).await;
}

/// A server saying "there is new mail" must become a queued read.
#[tokio::test]
async fn a_new_mail_notification_queues_a_read() {
    let (sync, mut trigger) = sync_channel();
    let source = Arc::new(ScriptedIdle::new(vec![new_mail(), stopped()]));
    let (_signal, shutdown) = Signal::new();

    idle_loop(source.clone(), sync, backoff(), shutdown).await;

    assert_eq!(trigger.try_recv(), Ok(()), "notification was not queued");
    assert_eq!(source.closes(), 1, "the session was not closed on exit");
}

/// The flag is sticky and checked before every wait, so a process already on
/// its way down never opens a connection just to log out of it.
#[tokio::test]
async fn a_stopped_flag_is_checked_before_opening_a_connection() {
    let (signal, shutdown) = Signal::new();
    signal.stop();
    // An empty script turns any extra `wait` into a panic rather than a hang.
    let source = Arc::new(ScriptedIdle::new(vec![]));

    run(source.clone(), shutdown).await;

    assert_eq!(source.closes(), 1);
}

/// A terminal refusal must stop the watcher immediately: retrying a server
/// that cannot idle, or a rejected password, is a log flood, not a recovery.
#[tokio::test]
async fn an_unsupported_server_stops_the_watcher_at_once() {
    let source = Arc::new(ScriptedIdle::new(vec![unsupported("no IDLE here")]));
    let (_signal, shutdown) = Signal::new();

    run(source.clone(), shutdown).await;

    assert_eq!(source.closes(), 1);
}

/// A dropped connection is retried, and the retry is actually delayed — the
/// difference between a reconnect and a hot loop against a down server.
#[tokio::test(start_paused = true)]
async fn a_transient_failure_is_retried_after_a_backoff() {
    let started = tokio::time::Instant::now();
    let source = Arc::new(ScriptedIdle::new(vec![
        transient("connection lost"),
        stopped(),
    ]));
    let (_signal, shutdown) = Signal::new();

    run(source.clone(), shutdown).await;

    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "retried without waiting: {:?}",
        started.elapsed()
    );
    assert_eq!(source.closes(), 1, "loop did not survive the failure");
}

/// One pending read absorbs the notifications that arrive while it is queued;
/// that coalescing is what stops a burst of `EXISTS` from becoming a burst of
/// `FETCH` commands.
#[test]
fn notifications_coalesce_while_one_is_pending() {
    let (sync, mut trigger) = sync_channel();

    assert!(sync.notify(), "the first notification must be accepted");
    assert!(
        !sync.notify(),
        "the second must be absorbed rather than queued"
    );
    assert_eq!(trigger.try_recv(), Ok(()));
    assert!(sync.notify(), "and the channel is free again");
}

/// The safety net fires on schedule and leaves when asked, without ever
/// blocking on anything but its own timer.
#[tokio::test(start_paused = true)]
async fn the_fallback_poller_nudges_the_reader_and_then_stops() {
    let (sync, mut trigger) = sync_channel();
    let (signal, shutdown) = Signal::new();
    let watcher = tokio::spawn(fallback::fallback_loop(
        sync,
        Duration::from_secs(90),
        shutdown,
    ));

    // The guard must outlast the period: with paused time the runtime jumps to
    // its next deadline, so a shorter guard would trip before the poller does.
    tokio::time::timeout(Duration::from_secs(300), trigger.recv())
        .await
        .expect("fallback poller never nudged the reader");

    signal.stop();
    watcher.await.expect("fallback poller panicked");
}
