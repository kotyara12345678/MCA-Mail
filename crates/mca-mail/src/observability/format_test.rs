//! Unit tests for the pretty terminal formatter.

use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;

use super::format::PrettyFormat;
use super::{Correlation, EVENT_TARGET};

type Buf = Arc<Mutex<Vec<u8>>>;

#[derive(Clone)]
struct BufWriter(Buf);

impl std::io::Write for BufWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for BufWriter {
    type Writer = BufWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Run `f` with the pretty formatter writing into an in-memory buffer.
fn capture(f: impl FnOnce()) -> String {
    let buf: Buf = Arc::new(Mutex::new(Vec::new()));
    let layer = tracing_subscriber::fmt::layer()
        .event_format(PrettyFormat)
        .with_ansi(false)
        .with_writer(BufWriter(buf.clone()));
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, f);
    let raw = buf.lock().unwrap().clone();
    String::from_utf8_lossy(&raw).to_string()
}

#[test]
fn pretty_line_is_time_level_message_fields() {
    let out = capture(|| {
        tracing::info!(
            target: EVENT_TARGET,
            email_id = "1842",
            agent = "spam",
            duration_ms = 731u64,
            "agent_completed"
        );
    });
    let line = out.trim_end();
    let re = regex::Regex::new(
        r"^\d{2}:\d{2}:\d{2} INFO  agent_completed email_id=1842 agent=spam duration_ms=731$",
    )
    .unwrap();
    assert!(re.is_match(line), "got: {line}");
}

#[test]
fn warn_and_error_levels_align() {
    let out = capture(|| {
        tracing::warn!("queue_loop stalled");
        tracing::error!(error_type = "timeout", "http_request_failure");
    });
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines[0].ends_with("WARN  queue_loop stalled"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].ends_with("ERROR http_request_failure error_type=timeout"),
        "{}",
        lines[1]
    );
}

#[test]
fn display_fields_stay_unquoted() {
    let email_id = uuid::Uuid::new_v4();
    let out = capture(|| {
        tracing::info!(target: EVENT_TARGET, email_id = %email_id, "email_received");
    });
    let line = out.trim_end();
    assert!(
        line.ends_with(&format!("email_received email_id={email_id}")),
        "{line}"
    );
}

#[test]
fn correlation_uses_proc_prefix() {
    let email_id = uuid::Uuid::new_v4();
    let run_id = uuid::Uuid::new_v4();
    let c = Correlation::new(email_id, run_id);
    assert_eq!(c.email_id, email_id);
    assert_eq!(c.processing_id, format!("proc_{run_id}"));
}
