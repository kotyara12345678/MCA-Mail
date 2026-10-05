//! API contract: authentication and role gates.
//!
//! The security promise in docs/API.md — "all endpoints except /health and
//! /ready require X-API-Key" — is only real if the extractor rejects unknown
//! keys and role gates reject under-privileged ones. Both are verified here
//! against a real database.
//!
//! Set `MCA_TEST_DATABASE_URL` to run them; without it the module is skipped.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;

use mca_mail::api::{self, ApiState};
use mca_mail::config::{ApiSettings, DatabaseSettings};
use mca_mail::persistence::api_key_repo;

async fn test_pool() -> Option<PgPool> {
    let url = std::env::var("MCA_TEST_DATABASE_URL").ok()?;
    let settings = DatabaseSettings {
        url,
        max_connections: 4,
        min_connections: 0,
        auto_migrate: false,
        ..Default::default()
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&settings.url)
        .await
        .ok()?;
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    Some(pool)
}

async fn app(pg: PgPool) -> axum::Router {
    let state = Arc::new(ApiState::new(pg.clone()));
    api::build_router(&ApiSettings::default(), state)
}

/// Issue a key for the test database.
async fn mint(pg: &PgPool, name: &str, role: api_key_repo::Role) -> String {
    let (raw, _) = api_key_repo::create(pg, name, role, "api_contract", None)
        .await
        .expect("create key");
    raw
}

async fn call(app: axum::Router, method: &str, uri: &str, key: Option<&str>) -> StatusCode {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(k) = key {
        builder = builder.header("X-API-Key", k);
    }
    let response = app
        .oneshot(builder.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    response.status()
}

#[tokio::test]
async fn health_endpoints_are_public() {
    let Some(pg) = test_pool().await else { return };
    let app = app(pg).await;
    assert_eq!(
        call(app.clone(), "GET", "/health", None).await,
        StatusCode::OK
    );
    assert_eq!(call(app, "GET", "/ready", None).await, StatusCode::OK);
}

/// Liveness and readiness must diverge when the database is gone: `/health`
/// keeps answering 200 because the process is still serving, while `/ready`
/// returns 503 — which is what `mca-mail healthcheck` probes, and therefore
/// what flips the container unhealthy in `docker compose ps`. Needs no
/// database of its own: the pool points somewhere nothing listens.
#[tokio::test]
async fn readiness_reports_a_dead_database_but_liveness_does_not() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://mca:mca@127.0.0.1:55432/mca_mail_ci7")
        .expect("lazy pool");
    let app = api::build_router(&ApiSettings::default(), Arc::new(ApiState::new(pool)));
    assert_eq!(
        call(app.clone(), "GET", "/health", None).await,
        StatusCode::OK
    );
    assert_eq!(
        call(app, "GET", "/ready", None).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn api_rejects_missing_and_unknown_keys() {
    let Some(pg) = test_pool().await else { return };
    let app = app(pg).await;
    assert_eq!(
        call(app.clone(), "GET", "/api/v1/emails", None).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(app, "GET", "/api/v1/emails", Some("mca_deadbeef")).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn viewer_can_read_but_not_approve() {
    let Some(pg) = test_pool().await else { return };
    let viewer = mint(&pg, "viewer", api_key_repo::Role::Viewer).await;
    let id = uuid::Uuid::new_v4();

    let app = app(pg).await;
    assert_eq!(
        call(app.clone(), "GET", "/api/v1/emails", Some(&viewer)).await,
        StatusCode::OK
    );
    assert_eq!(
        call(
            app,
            "POST",
            &format!("/api/v1/emails/{id}/approve"),
            Some(&viewer)
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn manager_can_approve() {
    let Some(pg) = test_pool().await else { return };
    let manager = mint(&pg, "manager", api_key_repo::Role::Manager).await;
    let id = uuid::Uuid::new_v4();

    let app = app(pg).await;
    assert_eq!(
        call(
            app,
            "POST",
            &format!("/api/v1/emails/{id}/approve"),
            Some(&manager)
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn operator_gates_reprocess() {
    let Some(pg) = test_pool().await else { return };
    let viewer = mint(&pg, "viewer2", api_key_repo::Role::Viewer).await;
    let operator = mint(&pg, "operator", api_key_repo::Role::Operator).await;
    let id = uuid::Uuid::new_v4();

    let app = app(pg).await;
    assert_eq!(
        call(
            app.clone(),
            "POST",
            &format!("/api/v1/emails/{id}/reprocess"),
            Some(&viewer)
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            app,
            "POST",
            &format!("/api/v1/emails/{id}/reprocess"),
            Some(&operator)
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn revoked_key_is_rejected() {
    let Some(pg) = test_pool().await else { return };
    let key = mint(&pg, "doomed", api_key_repo::Role::Viewer).await;
    let listed = api_key_repo::list(&pg).await.expect("list");
    let row = listed
        .iter()
        .find(|r| r.name == "doomed" && r.is_active)
        .expect("the key just minted");
    api_key_repo::revoke(&pg, row.id).await.expect("revoke");

    let app = app(pg).await;
    assert_eq!(
        call(app, "GET", "/api/v1/emails", Some(&key)).await,
        StatusCode::UNAUTHORIZED
    );
}
