//! Pretty terminal format: `HH:MM:SS LEVEL message field=value ...`.

use std::fmt;

use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::{FormatEvent, Writer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::LookupSpan;

use super::capture::Captured;

/// Compact single-line formatter used when `LOG_FORMAT` is not `json`.
///
/// Example: `18:03:21 INFO  agent_completed email=1842 agent=spam ...`.
pub struct PrettyFormat;

impl<S, N> FormatEvent<S, N> for PrettyFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::format::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let cap = Captured::from_event(event);
        let ts = chrono::Local::now().format("%H:%M:%S");
        write!(writer, "{ts} {:<5} ", event.metadata().level().as_str())?;
        if let Some(msg) = &cap.message {
            write!(writer, "{msg}")?;
        }
        for (name, val) in &cap.fields {
            write!(writer, " {name}={}", val.as_display())?;
        }
        writeln!(writer)
    }
}
