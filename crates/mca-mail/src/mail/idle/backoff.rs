//! Exponential backoff with jitter for IDLE reconnects.
//!
//! Exponential alone would have every replica of a service retry a flapping
//! server at the same instant; the jitter is what turns a thundering herd back
//! into independent clients. The cap is what stops an overnight outage from
//! turning into a "reconnect in four hours" bug.

use std::time::Duration;

use rand::Rng;

/// Consecutive failures never shift by more than this, so `min << shift`
/// cannot overflow before it has already hit the cap.
const MAX_SHIFT: u32 = 16;

#[derive(Debug, Clone)]
pub struct Backoff {
    min: Duration,
    max: Duration,
    jitter: f64,
    attempt: u32,
}

impl Backoff {
    /// `min` is the first delay, `max` the largest delay ever slept, and
    /// `jitter_percent` the spread applied to each delay either way.
    pub fn new(min: Duration, max: Duration, jitter_percent: u32) -> Self {
        let min = min.max(Duration::from_millis(1));
        Self {
            min,
            max: max.max(min),
            jitter: f64::from(jitter_percent.min(100)) / 100.0,
            attempt: 0,
        }
    }

    /// The undisturbed delay: `min * 2^attempt`, capped at `max`.
    ///
    /// Exposed separately from [`Self::next_delay`] so the schedule itself can
    /// be asserted without waiting for randomness to behave.
    pub fn base_delay(&self) -> Duration {
        let scaled = self
            .min
            .as_millis()
            .saturating_mul(1u128 << self.attempt.min(MAX_SHIFT));
        Duration::from_millis(scaled.min(self.max.as_millis()) as u64)
    }

    /// The delay to actually sleep: [`Self::base_delay`] nudged by jitter and
    /// still capped at `max`. Advances the attempt counter.
    pub fn next_delay(&mut self) -> Duration {
        let base = self.base_delay();
        self.attempt = self.attempt.saturating_add(1);
        jitter(base, self.jitter).min(self.max)
    }

    /// A successful cycle starts the schedule over, so one dropped connection
    /// after weeks of health does not inherit a five-minute penalty.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

/// Spread `base` by `±jitter`, never below one millisecond.
fn jitter(base: Duration, jitter: f64) -> Duration {
    if jitter <= 0.0 {
        return base;
    }
    let factor = 1.0 + rand::thread_rng().gen_range(-jitter..jitter);
    Duration::from_millis((base.as_millis() as f64 * factor).max(1.0) as u64)
}

#[cfg(test)]
#[path = "backoff_tests.rs"]
mod tests;
