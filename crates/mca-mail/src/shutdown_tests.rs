use std::time::Duration;

use super::{pause, stopping, Signal};

/// A freshly built signal stops nobody: `stopping` must stay pending until it
/// is actually raised, which is what lets a worker sleep out a backoff.
#[tokio::test(start_paused = true)]
async fn a_new_signal_does_not_stop_anything() {
    let (signal, mut rx) = Signal::new();
    let pending = tokio::time::timeout(Duration::from_secs(60), stopping(&mut rx)).await;

    assert!(pending.is_err(), "must not resolve before stop()");

    signal.stop();
    assert!(stopping(&mut rx).await, "must stop once stop() is called");
}

/// A dropped sender is treated as a stop: the owning task is gone, so staying
/// alive would mean waiting for a wake-up that can never arrive.
#[tokio::test]
async fn a_dropped_sender_stops_the_worker() {
    let (signal, mut rx) = Signal::new();
    drop(signal);
    assert!(stopping(&mut rx).await);
}

/// The backoff sleep must not keep a worker alive past shutdown.
#[tokio::test(start_paused = true)]
async fn a_pause_wakes_early_on_stop() {
    let (signal, mut rx) = Signal::new();
    let started = tokio::time::Instant::now();

    signal.stop();
    assert!(pause(&mut rx, Duration::from_secs(60)).await);

    assert!(
        started.elapsed() < Duration::from_secs(60),
        "a stop must not wait out the whole delay"
    );
}

/// Without a stop the delay is honoured exactly, which is what makes the
/// exponential reconnect schedule meaningful.
#[tokio::test(start_paused = true)]
async fn a_pause_waits_the_full_delay_when_nothing_stops_it() {
    let (_signal, mut rx) = Signal::new();
    let started = tokio::time::Instant::now();

    assert!(!pause(&mut rx, Duration::from_secs(7)).await);

    assert_eq!(started.elapsed(), Duration::from_secs(7));
}
