//! Observability: structured pipeline events, correlation ids and the SSE feed.
//!
//! Every lifecycle event is emitted through `tracing` with the dedicated
//! target [`EVENT_TARGET`]. The terminal renders it with the compact pretty
//! format (`HH:MM:SS LEVEL message field=value ...`), `LOG_FORMAT=json`
//! switches to newline-delimited JSON, and [`bus::BroadcastLayer`] mirrors
//! the same events to an in-memory channel that powers
//! `GET /api/events/stream`.
//!
//! Environment:
//! * `LOG_LEVEL` — fallback filter when `RUST_LOG` is unset (default `info`);
//! * `LOG_FORMAT` — `pretty` (default) or `json` (production).

pub mod actions;
pub mod agents;
pub mod bus;
mod capture;
pub mod errors;
pub mod format;
pub mod http;
pub mod lifecycle;
pub mod llm;
pub mod queue;
pub mod system;
pub mod tools;

#[cfg(test)]
#[path = "bus_test.rs"]
mod bus_test;
#[cfg(test)]
#[path = "events_test.rs"]
mod events_test;
#[cfg(test)]
#[path = "format_test.rs"]
mod format_test;
#[cfg(test)]
#[path = "http_events_test.rs"]
mod http_events_test;
#[cfg(test)]
#[path = "system_events_test.rs"]
mod system_events_test;
#[cfg(test)]
#[path = "test_util.rs"]
mod test_util;
#[cfg(test)]
#[path = "tools_events_test.rs"]
mod tools_events_test;

pub use bus::BusEvent;

/// Tracing target used by all observability events (SSE filter key).
pub const EVENT_TARGET: &str = "mca::obs";

/// Correlation identifiers attached to every pipeline event of one email.
///
/// `processing_id` is the run identifier in the `proc_` form used in logs
/// and the SSE stream; it correlates log lines, API reprocess calls and
/// audit rows of a single pipeline run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correlation {
    pub email_id: crate::domain::EmailId,
    pub processing_id: String,
}

impl Correlation {
    /// Build the correlation pair from a run and its email.
    pub fn new(email_id: crate::domain::EmailId, run_id: crate::domain::RunId) -> Self {
        Self {
            email_id,
            processing_id: format!("proc_{run_id}"),
        }
    }
}

/// Install the global tracing subscriber (call once from `main`).
///
/// Precedence for the level: `RUST_LOG` > `LOG_LEVEL` > `info`.
/// `LOG_FORMAT=json` selects the JSON formatter, anything else (the default)
/// selects the human-readable format.
pub fn init() {
    use tracing_subscriber::prelude::*;

    let filter = match tracing_subscriber::EnvFilter::try_from_default_env() {
        Ok(f) => f,
        Err(_) => {
            let level = std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
            tracing_subscriber::EnvFilter::new(level)
        }
    };

    let json = std::env::var("LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    let base = tracing_subscriber::registry().with(bus::BroadcastLayer);
    if json {
        base.with(tracing_subscriber::fmt::layer().json().with_filter(filter))
            .init();
    } else {
        base.with(
            tracing_subscriber::fmt::layer()
                .event_format(format::PrettyFormat)
                .with_filter(filter),
        )
        .init();
    }
}
