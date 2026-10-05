//! IMAP IDLE tuning, kept apart from the connection settings it sits beside.
//!
//! IDLE is an opt-in extra on top of polling; these values only shape how it
//! behaves, so an operator who switches `MAIL_IDLE` off never has to reason
//! about them.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// RFC 2177 advises re-issuing IDLE at least every 29 minutes, because a server
/// with an inactivity timeout may otherwise log the client off silently.
const RFC_REISSUE_SECONDS: u64 = 29 * 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IdleSettings {
    /// End and re-issue IDLE at least this often. Capped at [`RFC_REISSUE_SECONDS`].
    pub wait_seconds: u64,
    /// Delay before the first reconnect after a failed IDLE cycle.
    pub reconnect_min_seconds: u64,
    /// Upper bound on that delay, however many attempts have failed.
    pub reconnect_max_seconds: u64,
    /// Random spread applied to each delay, as a percentage of it.
    pub jitter_percent: u32,
}

impl Default for IdleSettings {
    fn default() -> Self {
        Self {
            wait_seconds: RFC_REISSUE_SECONDS,
            reconnect_min_seconds: 1,
            reconnect_max_seconds: 300,
            jitter_percent: 20,
        }
    }
}

impl IdleSettings {
    /// The largest value `wait_seconds` may hold: staying under the server's
    /// inactivity window is what keeps an idle connection alive at all.
    pub const MAX_WAIT_SECONDS: u64 = RFC_REISSUE_SECONDS;

    /// How long one IDLE cycle may last before we end it and issue another.
    pub fn wait(&self) -> Duration {
        Duration::from_secs(self.wait_seconds.clamp(1, Self::MAX_WAIT_SECONDS))
    }

    /// Delay before the first reconnect after a failed IDLE cycle.
    pub fn reconnect_min(&self) -> Duration {
        Duration::from_secs(self.reconnect_min_seconds.max(1))
    }

    /// Upper bound on that delay, however many attempts have failed.
    pub fn reconnect_max(&self) -> Duration {
        Duration::from_secs(self.reconnect_max_seconds.max(1))
    }
}
