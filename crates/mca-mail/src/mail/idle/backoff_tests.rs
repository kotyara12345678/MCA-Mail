use std::time::Duration;

use super::Backoff;

fn backoff() -> Backoff {
    Backoff::new(
        Duration::from_secs(1),
        Duration::from_secs(60),
        0, // jitter off, so the schedule itself is observable
    )
}

/// The first failure waits `min`, and each further one doubles — until the cap.
#[test]
fn the_schedule_doubles_and_then_stops_at_the_cap() {
    let mut backoff = backoff();
    let expected = [1, 2, 4, 8, 16, 32, 60, 60, 60];
    for seconds in expected {
        assert_eq!(backoff.base_delay(), Duration::from_secs(seconds));
        backoff.next_delay();
    }
    assert_eq!(backoff.attempt(), expected.len() as u32);
}

/// `min` must never be allowed to be zero: that is a busy loop, not a backoff.
#[test]
fn a_zero_minimum_is_raised_to_a_millisecond() {
    let mut backoff = Backoff::new(Duration::ZERO, Duration::ZERO, 0);
    assert!(backoff.base_delay() >= Duration::from_millis(1));
    assert!(backoff.next_delay() >= Duration::from_millis(1));
}

/// A `max` below `min` would make the schedule go backwards; the larger wins.
#[test]
fn a_max_below_the_minimum_is_raised_to_the_minimum() {
    let mut backoff = Backoff::new(Duration::from_secs(30), Duration::from_secs(5), 0);
    assert_eq!(backoff.base_delay(), Duration::from_secs(30));
    assert_eq!(backoff.next_delay(), Duration::from_secs(30));
}

/// Jitter must stay inside its band and must never exceed the cap, or a burst
/// of failures could sleep far longer than the operator asked for.
#[test]
fn jitter_stays_inside_its_band_and_under_the_cap() {
    let mut backoff = Backoff::new(
        Duration::from_secs(10),
        Duration::from_secs(10),
        50, // ±50%
    );
    for _ in 0..200 {
        let delay = backoff.next_delay();
        assert!(delay <= Duration::from_secs(10), "cap respected: {delay:?}");
        assert!(delay >= Duration::from_secs(5), "lower band: {delay:?}");
        backoff.reset();
    }
}

/// Reaching the cap and then succeeding must start over from the minimum.
#[test]
fn a_reset_starts_the_schedule_over() {
    let mut backoff = backoff();
    for _ in 0..5 {
        backoff.next_delay();
    }
    assert!(backoff.base_delay() > Duration::from_secs(1));
    backoff.reset();
    assert_eq!(backoff.base_delay(), Duration::from_secs(1));
}
