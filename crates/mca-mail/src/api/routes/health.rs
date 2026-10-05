//! Health and readiness endpoints.
//!
//! `/health` is liveness: the process is answering HTTP, so it always returns
//! 200 once the server is up. `/ready` is readiness: it returns 503 until the
//! database answers, because that is the signal a healthcheck, a proxy or a
//! load balancer actually looks at.
//!
//! Both probe the pool on every call — a `db_health` snapshot taken during
//! bootstrap would keep reporting "connected" long after the pool stopped
//! working, which is exactly when an operator needs to be told. The probe is
//! bounded: a healthcheck that can hang is worse than one that answers "no".

use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

use crate::api::ApiState;
use crate::persistence::pool;

/// Upper bound on a single liveness/readiness round-trip. Docker's HEALTHCHECK
/// gives up after 5s; answering well inside that keeps the probe useful.
const DB_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

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

async fn probe_db(state: &Arc<ApiState>) -> bool {
    match tokio::time::timeout(DB_PROBE_TIMEOUT, pool::health(&state.pool)).await {
        Ok(health) => health.reachable,
        Err(_) => false,
    }
}

async fn health(State(state): State<Arc<ApiState>>) -> Json<HealthResponse> {
    let db_ok = probe_db(&state).await;
    Json(HealthResponse {
        status: "ok",
        uptime_seconds: state.start_time.elapsed().as_secs(),
        version: env!("CARGO_PKG_VERSION"),
        database: if db_ok { "connected" } else { "disconnected" },
    })
}

async fn ready(State(state): State<Arc<ApiState>>) -> (StatusCode, Json<ReadyResponse>) {
    let db_ok = probe_db(&state).await;
    let code = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        code,
        Json(ReadyResponse {
            status: if db_ok { "ready" } else { "not_ready" },
            database: if db_ok { "connected" } else { "disconnected" },
        }),
    )
}

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
}
