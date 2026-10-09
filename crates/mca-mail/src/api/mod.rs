//! REST API layer — Axum-based HTTP server.

pub mod auth;
pub mod errors;
mod http_log;
pub mod routes;

use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;

use crate::api::routes::{email_routes, events, health, voice};
use crate::config::ApiSettings;
use crate::orchestration::Orchestrator;

/// Shared application state accessible from handlers.
pub struct ApiState {
    pub pool: sqlx::PgPool,
    pub orchestrator: Option<Arc<Orchestrator>>,
    pub start_time: tokio::time::Instant,
}

impl ApiState {
    pub fn new(pool: sqlx::PgPool) -> Self {
        ApiState {
            pool,
            orchestrator: None,
            start_time: tokio::time::Instant::now(),
        }
    }
}

pub type SharedState = Arc<ApiState>;

/// Build the Axum router with all routes.
///
/// `http_log` is the outermost layer (request id, 30s timeout, http events);
/// it runs after routing so the matched path pattern is available. The SSE
/// routes are merged afterwards on purpose — a stream must never be cut by
/// the request timeout.
pub fn build_router(_settings: &ApiSettings, state: SharedState) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(email_routes::routes())
        .merge(voice::routes())
        .layer(CorsLayer::permissive())
        .layer(axum::middleware::from_fn(http_log::log_request))
        .merge(events::routes())
        .with_state(state)
}
