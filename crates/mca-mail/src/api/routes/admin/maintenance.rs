//! One-shot maintenance actions the panel offers: wake the mail reader now,
//! unstick processing that a restart interrupted, clear old events, and put a
//! failed send back into the queue.

use axum::{
    extract::{Path, Query, State},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::auth::AuthenticatedRequest;
use crate::api::SharedState;
use crate::error::AppError;
use crate::persistence::{api_key_repo::Role, event_repo, outbox_repo};

#[derive(Debug, Serialize)]
pub struct PollNowView {
    /// `false` means no mail loop is running (no mailbox configured): the
    /// panel then shows the button as unavailable instead of pretending.
    pub requested: bool,
}

/// Poke the one-slot trigger the reader listens on. When a fetch is already
/// queued the notify is a no-op — that is the coalescing contract, so any
/// outcome here means "mail will be read promptly".
async fn poll_now(
    State(state): State<SharedState>,
    auth: AuthenticatedRequest,
) -> Result<Json<PollNowView>, AppError> {
    auth.require(Role::Operator)?;
    let requested = {
        let guard = state.sync.read().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(sync) => {
                sync.notify();
                true
            }
            None => false,
        }
    };
    Ok(Json(PollNowView { requested }))
}

#[derive(Debug, Serialize)]
pub struct RecoverView {
    pub recovered: u64,
}

/// Flip emails a crashed worker left in `processing` back to `pending`.
async fn recover(
    State(state): State<SharedState>,
    auth: AuthenticatedRequest,
) -> Result<Json<RecoverView>, AppError> {
    auth.require(Role::Operator)?;
    let recovered =
        crate::application::workers::recovery::recover_stuck_emails(&state.pool).await?;
    Ok(Json(RecoverView { recovered }))
}

#[derive(Debug, Deserialize)]
pub struct PurgeQuery {
    pub older_than_days: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct PurgeView {
    pub purged: u64,
}

/// Drop processing events older than the window (default 30 days).
async fn purge_events(
    State(state): State<SharedState>,
    Query(query): Query<PurgeQuery>,
    auth: AuthenticatedRequest,
) -> Result<Json<PurgeView>, AppError> {
    auth.require(Role::Operator)?;
    let days = query.older_than_days.unwrap_or(30).clamp(1, 3650);
    let purged = event_repo::purge_events(&state.pool, days).await?;
    Ok(Json(PurgeView { purged }))
}

#[derive(Debug, Serialize)]
pub struct RetryView {
    pub queued: bool,
}

/// Requeue one failed or held outbox row; anything else is 404 so the panel
/// can tell "gone" from "wrong state".
async fn outbox_retry(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<RetryView>, AppError> {
    auth.require(Role::Operator)?;
    if !outbox_repo::retry(&state.pool, id).await? {
        return Err(AppError::NotFound(format!(
            "outbox row {id} is not failed or held"
        )));
    }
    Ok(Json(RetryView { queued: true }))
}

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/api/admin/ops/poll-now", post(poll_now))
        .route("/api/admin/ops/recover", post(recover))
        .route("/api/admin/ops/purge-events", post(purge_events))
        .route("/api/admin/outbox/{id}/retry", post(outbox_retry))
}
