//! Health and readiness endpoints.

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;

use crate::api::ApiState;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    uptime_seconds: u64,
    version: &'static str,
    database: &'static str,
}

#[derive(Serialize)]
struct ReadyResponse {
    status: &'static str,
    database: &'static str,
}

async fn health(State(state): State<Arc<ApiState>>) -> Json<HealthResponse> {
    let uptime = state.start_time.elapsed().as_secs();
    Json(HealthResponse {
        status: "ok",
        uptime_seconds: uptime,
        version: env!("CARGO_PKG_VERSION"),
        database: if state.db_health.reachable {
            "connected"
        } else {
            "disconnected"
        },
    })
}

async fn ready(State(state): State<Arc<ApiState>>) -> Json<ReadyResponse> {
    let db_ok = state.db_health.reachable;
    Json(ReadyResponse {
        status: if db_ok { "ready" } else { "not_ready" },
        database: if db_ok { "connected" } else { "disconnected" },
    })
}

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
}
