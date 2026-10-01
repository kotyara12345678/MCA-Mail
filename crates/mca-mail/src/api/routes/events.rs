//! Live observability feed: SSE stream of pipeline events + HTML viewer.

use std::convert::Infallible;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use futures::stream;

use crate::api::SharedState;
use crate::observability::bus;

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/api/events/stream", get(event_stream))
        .route("/events", get(events_page))
}

/// Server-sent events: one JSON message per `mca::obs` tracing event.
async fn event_stream() -> Sse<impl stream::Stream<Item = Result<Event, Infallible>>> {
    let receiver = bus::subscribe();
    let events = stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let sse = Event::default().data(event.json);
                    return Some((Ok(sse), receiver));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(events).keep_alive(KeepAlive::default())
}

async fn events_page() -> Html<&'static str> {
    Html(PAGE)
}

const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>MCA Mail — live events</title>
<style>
  body { font-family: ui-monospace, monospace; background: #0b0e14; color: #cdd6f4;
         margin: 0; padding: 1rem; }
  h1 { font-size: 1rem; color: #89b4fa; }
  #status { color: #a6adc8; font-size: .85rem; margin-bottom: .75rem; }
  #log { white-space: pre-wrap; word-break: break-all; font-size: .8rem;
         line-height: 1.5; }
  .error { color: #f38ba8; } .warn { color: #f9e2af; }
</style>
</head>
<body>
<h1>MCA Mail — live events</h1>
<div id="status">connecting…</div>
<div id="log"></div>
<script>
const status = document.getElementById("status");
const log = document.getElementById("log");
const source = new EventSource("/api/events/stream");
source.onopen = () => status.textContent = "connected";
source.onerror = () => status.textContent = "reconnecting…";
source.onmessage = (e) => {
  try {
    const d = JSON.parse(e.data);
    const line = document.createElement("div");
    line.className = d.level === "ERROR" ? "error" : d.level === "WARN" ? "warn" : "";
    line.textContent = `${d.ts} ${d.level} ${d.event}  ` +
      Object.entries(d).filter(([k]) => !["ts", "level", "event"].includes(k))
        .map(([k, v]) => `${k}=${v}`).join(" ");
    log.prepend(line);
    while (log.childElementCount > 500) log.lastChild.remove();
  } catch (_) {
    const line = document.createElement("div");
    line.textContent = e.data;
    log.prepend(line);
  }
};
</script>
</body>
</html>"#;
