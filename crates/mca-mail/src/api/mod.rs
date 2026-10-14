//! REST API layer — Axum-based HTTP server.

pub mod auth;
pub mod errors;
mod http_log;
pub mod routes;

use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;

use crate::api::routes::{admin, email_routes, events, health, session, spa, voice};
use crate::config::{ApiSettings, AppConfig};
use crate::orchestration::Orchestrator;

/// The single pair of credentials the admin panel logs in with.
///
/// `None` disables the login route entirely (unit-style test apps), which is
/// safer than starting with a default password.
#[derive(Debug, Clone)]
pub struct AdminCredentials {
    pub login: String,
    pub password: String,
}

/// Shared application state accessible from handlers.
pub struct ApiState {
    pub pool: sqlx::PgPool,
    pub orchestrator: Option<Arc<Orchestrator>>,
    pub start_time: tokio::time::Instant,
    pub admin: Option<AdminCredentials>,
    /// Read-only configuration snapshot for the admin summary page.
    pub config: Option<Arc<AppConfig>>,
}

impl ApiState {
    pub fn new(pool: sqlx::PgPool) -> Self {
        ApiState {
            pool,
            orchestrator: None,
            start_time: tokio::time::Instant::now(),
            admin: None,
            config: None,
        }
    }

    /// Attach the panel login (builder style; production calls this once).
    pub fn with_admin(mut self, login: String, password: String) -> Self {
        self.admin = Some(AdminCredentials { login, password });
        self
    }

    /// Attach the config snapshot the summary page reads.
    pub fn with_config(mut self, config: Arc<AppConfig>) -> Self {
        self.config = Some(config);
        self
    }
}

pub type SharedState = Arc<ApiState>;

/// Build the Axum router with all routes.
///
/// `http_log` is the outermost layer (request id, 30s timeout, http events);
/// it runs after routing so the matched path pattern is available. The SSE
/// routes are merged afterwards on purpose — a stream must never be cut by
/// the request timeout. The SPA fallback answers unknown non-API GETs with
/// the panel shell so bookmarks work.
pub fn build_router(_settings: &ApiSettings, state: SharedState) -> Router {
    Router::new()
        .merge(session::routes())
        .merge(health::routes())
        .merge(email_routes::routes())
        .merge(admin::routes())
        .merge(voice::routes())
        .merge(spa::routes())
        .fallback(spa::fallback)
        .layer(CorsLayer::permissive())
        .layer(axum::middleware::from_fn(http_log::log_request))
        .merge(events::routes())
        .with_state(state)
}
