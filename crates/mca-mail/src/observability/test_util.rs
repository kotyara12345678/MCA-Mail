//! Shared test helpers: scoped subscriber with the broadcast layer.

use std::time::{Duration, Instant};

use tokio::sync::broadcast::error::TryRecvError;
use tracing_subscriber::layer::SubscriberExt;

use super::{bus, BusEvent};

const POLL: Duration = Duration::from_millis(5);

/// Run `f` with the broadcast layer installed as the current subscriber.
pub(crate) fn run(f: impl FnOnce()) {
    let subscriber = tracing_subscriber::registry().with(bus::BroadcastLayer);
    tracing::subscriber::with_default(subscriber, f);
}

/// Wait until an event whose JSON payload contains `marker` arrives.
///
/// Other tests share the global bus, so listeners must select their own
/// events instead of assuming the next message is theirs.
pub(crate) fn await_marker(
    rx: &mut tokio::sync::broadcast::Receiver<BusEvent>,
    marker: &str,
) -> BusEvent {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if Instant::now() >= deadline {
            panic!("event containing {marker:?} not seen");
        }
        match rx.try_recv() {
            Ok(ev) if ev.json.contains(marker) => return ev,
            Ok(_) => continue,
            Err(TryRecvError::Empty | TryRecvError::Lagged(_)) => std::thread::sleep(POLL),
            Err(TryRecvError::Closed) => panic!("event bus closed"),
        }
    }
}

/// Assert that no event containing `marker` arrives within `wait`.
pub(crate) fn assert_absent(
    rx: &mut tokio::sync::broadcast::Receiver<BusEvent>,
    marker: &str,
    wait: Duration,
) {
    let deadline = Instant::now() + wait;
    loop {
        if Instant::now() >= deadline {
            return;
        }
        match rx.try_recv() {
            Ok(ev) if ev.json.contains(marker) => panic!("unexpected event: {}", ev.json),
            Ok(_) => continue,
            Err(TryRecvError::Empty | TryRecvError::Lagged(_)) => std::thread::sleep(POLL),
            Err(TryRecvError::Closed) => return,
        }
    }
}
