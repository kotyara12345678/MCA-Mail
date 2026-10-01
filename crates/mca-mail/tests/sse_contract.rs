//! SSE contract: `GET /api/events/stream` and the `/events` viewer page.
//!
//! No database needed — the pool is lazy and the health state is static.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use tracing_subscriber::prelude::*;

use mca_mail::api::{self, ApiState};
use mca_mail::config::ApiSettings;
use mca_mail::persistence::pool;

fn app() -> axum::Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://mca:mca@127.0.0.1:55432/mca_mail_ci7")
        .expect("lazy pool");
    let health = pool::Health {
        reachable: false,
        latency_ms: 0,
        detail: None,
    };
    api::build_router(
        &ApiSettings::default(),
        Arc::new(ApiState::new(pool, health)),
    )
}

async fn get(app: axum::Router, uri: &str) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response")
}

#[tokio::test]
async fn events_page_is_served_as_html() {
    let response = get(app(), "/events").await;
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .expect("content-type");
    assert!(content_type.to_str().unwrap().starts_with("text/html"));
}

#[tokio::test]
async fn event_stream_delivers_bus_events_as_json_data() {
    let response = get(app(), "/api/events/stream").await;
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .expect("content-type");
    assert!(content_type
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));

    let subscriber =
        tracing_subscriber::registry().with(mca_mail::observability::bus::BroadcastLayer);
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::info!(
        target: mca_mail::observability::EVENT_TARGET,
        marker = "sse-contract-marker",
        "sse_probe"
    );
    drop(_guard);

    let mut body = response.into_body();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut seen = String::new();
    while !seen.contains("sse-contract-marker") {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!remaining.is_zero(), "marker never arrived: {seen}");
        let frame = tokio::time::timeout(remaining, body.frame())
            .await
            .expect("frame deadline")
            .expect("stream still open")
            .expect("frame decoded");
        if let Ok(bytes) = frame.into_data() {
            seen.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
    assert!(seen.contains("\"event\":\"sse_probe\""), "{seen}");
}
