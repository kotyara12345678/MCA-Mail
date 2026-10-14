//! Operational catalogs for the panel: events, leads, the send queue, and
//! per-email run traces.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::auth::AuthenticatedRequest;
use crate::api::SharedState;
use crate::error::AppError;
use crate::persistence::event_repo::{self, EventFilter, EventRow};
use crate::persistence::lead_repo::{self, LeadFilter, LeadSummary};
use crate::persistence::outbox_repo::{self, OutboxSummary};
use crate::persistence::run_repo::{self, AgentRunSummary, RunSummary};

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub total: i64,
}

pub fn page(query: &PageQuery) -> (i64, i64) {
    (
        query.limit.unwrap_or(50).clamp(1, 200),
        query.offset.unwrap_or(0).max(0),
    )
}

async fn events(
    State(state): State<SharedState>,
    Query(query): Query<PageQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<Paged<EventRow>>, AppError> {
    let (limit, offset) = page(&query);
    let filter = EventFilter {
        limit,
        offset,
        ..Default::default()
    };
    let total = event_repo::count(&state.pool, &filter).await?;
    let items = event_repo::list(&state.pool, &filter).await?;
    Ok(Json(Paged { items, total }))
}

async fn leads(
    State(state): State<SharedState>,
    Query(query): Query<PageQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<Paged<LeadSummary>>, AppError> {
    let (limit, offset) = page(&query);
    let filter = LeadFilter {
        limit,
        offset,
        ..Default::default()
    };
    let total = lead_repo::count(&state.pool, &filter).await?;
    let items = lead_repo::list(&state.pool, &filter).await?;
    Ok(Json(Paged { items, total }))
}

async fn outbox(
    State(state): State<SharedState>,
    Query(query): Query<PageQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<Vec<OutboxSummary>>, AppError> {
    let (limit, _) = page(&query);
    Ok(Json(outbox_repo::recent(&state.pool, limit).await?))
}

#[derive(Debug, serde::Serialize)]
pub struct EmailRunsView {
    /// Pipeline runs (state machine attempts).
    pub runs: Vec<RunSummary>,
    /// Individual LLM calls across those runs, newest first.
    pub agent_runs: Vec<AgentRunSummary>,
}

async fn email_runs(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(query): Query<PageQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<EmailRunsView>, AppError> {
    let (limit, _) = page(&query);
    let runs = run_repo::list(
        &state.pool,
        &run_repo::RunFilter {
            email_id: Some(id),
            limit,
            ..Default::default()
        },
    )
    .await?;
    let agent_runs = run_repo::agent_runs_for_email(&state.pool, id, limit).await?;
    Ok(Json(EmailRunsView { runs, agent_runs }))
}

async fn email_events(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(query): Query<PageQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<Vec<EventRow>>, AppError> {
    let (limit, offset) = page(&query);
    let filter = EventFilter {
        email_id: Some(id),
        limit,
        offset,
        ..Default::default()
    };
    Ok(Json(event_repo::list(&state.pool, &filter).await?))
}

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/api/admin/events", get(events))
        .route("/api/admin/leads", get(leads))
        .route("/api/admin/outbox", get(outbox))
        .route("/api/admin/emails/{id}/runs", get(email_runs))
        .route("/api/admin/emails/{id}/events", get(email_events))
}
