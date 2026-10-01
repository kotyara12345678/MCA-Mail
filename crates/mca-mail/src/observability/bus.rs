//! In-memory event bus feeding `GET /api/events/stream` (SSE).

use std::sync::OnceLock;

use serde_json::Value;
use tokio::sync::broadcast;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use super::capture::Captured;
use super::EVENT_TARGET;

/// Broadcast capacity: enough burst room without unbounded memory.
const BUS_CAPACITY: usize = 512;

/// One live event framed for SSE: the event name and its JSON payload.
#[derive(Debug, Clone)]
pub struct BusEvent {
    /// Value used as the SSE `event:` line.
    pub event: String,
    /// JSON object with `ts`, `level`, `event` and the event fields.
    pub json: String,
}

static BUS: OnceLock<broadcast::Sender<BusEvent>> = OnceLock::new();

fn sender() -> &'static broadcast::Sender<BusEvent> {
    BUS.get_or_init(|| broadcast::channel(BUS_CAPACITY).0)
}

/// Subscribe to the live stream (events published from now on).
pub fn subscribe() -> broadcast::Receiver<BusEvent> {
    sender().subscribe()
}

/// Forward observability events (`EVENT_TARGET` only) to SSE subscribers.
///
/// Registered as a tracing layer in [`super::init`]. Serialisation is skipped
/// entirely while nobody is connected.
pub struct BroadcastLayer;

impl<S> Layer<S> for BroadcastLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != EVENT_TARGET {
            return;
        }
        let tx = sender();
        if tx.receiver_count() == 0 {
            return;
        }
        let cap = Captured::from_event(event);
        let name = cap.message.clone().unwrap_or_else(|| "event".to_string());
        let mut map = serde_json::Map::new();
        map.insert("ts".into(), Value::String(chrono::Utc::now().to_rfc3339()));
        map.insert(
            "level".into(),
            Value::String(event.metadata().level().to_string()),
        );
        map.insert("event".into(), Value::String(name.clone()));
        for (k, v) in &cap.fields {
            map.insert(k.clone(), v.as_json());
        }
        let _ = tx.send(BusEvent {
            event: name,
            json: Value::Object(map).to_string(),
        });
    }
}
