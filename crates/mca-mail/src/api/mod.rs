//! REST API layer — Axum-based HTTP server.

pub mod auth;
pub mod errors;
pub mod routes;

use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderName;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::request_id::MakeRequestUuid;
use tower_http::request_id::SetRequestIdLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::api::routes::{email_routes, health};
use crate::config::ApiSettings;
use crate::orchestration::Orchestrator;
use crate::persistence::pool::Health;

/// Shared application state accessible from handlers.
pub struct ApiState {
    pub pool: sqlx::PgPool,
    pub orchestrator: Option<Arc<Orchestrator>>,
    pub start_time: tokio::time::Instant,
    pub db_health: Health,
}

impl ApiState {
    pub fn new(pool: sqlx::PgPool, db_health: Health) -> Self {
        ApiState {
            pool,
            orchestrator: None,
            start_time: tokio::time::Instant::now(),
            db_health,
        }
    }
}

pub type SharedState = Arc<ApiState>;

/// Build the Axum router with all routes.
pub fn build_router(_settings: &ApiSettings, state: SharedState) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");
    Router::new()
        .merge(health::routes())
        .merge(email_routes::routes())
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
