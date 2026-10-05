//! HTTP logging middleware: request id header and `http_request` events.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use tracing_subscriber::prelude::*;

use mca_mail::api::{self, ApiState};
use mca_mail::config::ApiSettings;
use mca_mail::observability::bus;

fn app() -> axum::Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://mca:mca@127.0.0.1:55432/mca_mail_ci7")
        .expect("lazy pool");
    api::build_router(&ApiSettings::default(), Arc::new(ApiState::new(pool)))
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
async fn responses_carry_a_request_id() {
    let response = get(app(), "/health").await;
    assert_eq!(response.status(), StatusCode::OK);
    let request_id = response
        .headers()
        .get("x-request-id")
        .expect("x-request-id header");
    assert!(!request_id.to_str().unwrap().is_empty());
}

#[tokio::test]
async fn matched_requests_emit_http_request_events() {
    let mut receiver = bus::subscribe();
    let subscriber = tracing_subscriber::registry().with(bus::BroadcastLayer);
    let _guard = tracing::subscriber::set_default(subscriber);

    let response = get(app(), "/health").await;
    assert_eq!(response.status(), StatusCode::OK);

    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match receiver.recv().await {
                Ok(event)
                    if event.event == "http_request"
                        && event.json.contains("\"path\":\"/health\"") =>
                {
                    return event
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("event bus closed")
                }
            }
        }
    })
    .await
    .expect("http_request event");
    assert!(event.json.contains("\"status\":200"), "{}", event.json);
    assert!(event.json.contains("\"request_id\""), "{}", event.json);
    assert!(event.json.contains("\"duration_ms\""), "{}", event.json);
}
