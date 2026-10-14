//! The four verbs a manager clicks on an email: reprocess, review, approve,
//! reject.
//!
//! All four answer `200` with the same `ActionResponse`, including for an id
//! that does not exist (the message is the acknowledgement either way) —
//! scripts fire these at id lists, and one stale id must not look like a
//! failure. What differs is what happens when the row is there:
//!
//! - reprocess → back to `pending` (unless a worker owns it right now);
//! - review → `needs_review`;
//! - approve → `processed`;
//! - reject → `quarantined`.

use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use uuid::Uuid;

use crate::api::auth::AuthenticatedRequest;
use crate::api::ApiState;
use crate::domain::EmailStatus;
use crate::error::AppError;
use crate::persistence::api_key_repo::Role;
use crate::persistence::email_repo;

use super::email_routes::ActionResponse;

type AxumState = std::sync::Arc<ApiState>;

/// Stable message shapes — the API contract (`tests/api_contract.rs`) keys on
/// them, so render through these helpers rather than format literals.
fn ok(id: Uuid, message: impl FnOnce(Uuid) -> String) -> Json<ActionResponse> {
    Json(ActionResponse {
        success: true,
        message: message(id),
    })
}

async fn reprocess(
    State(state): State<AxumState>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AppError> {
    auth.require(Role::Operator)?;
    transition(&state, id, None).await?;
    Ok(ok(id, |id| format!("reprocessing scheduled for {id}")))
}

async fn review(
    State(state): State<AxumState>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AppError> {
    auth.require(Role::Operator)?;
    transition(&state, id, Some(EmailStatus::NeedsReview)).await?;
    Ok(ok(id, |id| format!("email {id} flagged for review")))
}

async fn approve(
    State(state): State<AxumState>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AppError> {
    auth.require(Role::Manager)?;
    transition(&state, id, Some(EmailStatus::Processed)).await?;
    Ok(ok(id, |id| format!("email {id} approved")))
}

async fn reject(
    State(state): State<AxumState>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AppError> {
    auth.require(Role::Manager)?;
    transition(&state, id, Some(EmailStatus::Quarantined)).await?;
    Ok(ok(id, |id| format!("email {id} rejected")))
}

/// Apply an action to the row when it exists. `None` means requeue. A missing
/// row is not an error: the acknowledgement stands, nothing was changed.
async fn transition(
    state: &AxumState,
    id: Uuid,
    status: Option<EmailStatus>,
) -> Result<(), AppError> {
    let Some(_email) = email_repo::find(&state.pool, id).await? else {
        return Ok(());
    };
    match status {
        None => email_repo::requeue(&state.pool, id).await,
        Some(status) => email_repo::set_status(&state.pool, id, status, None).await,
    }
}

pub fn routes() -> Router<AxumState> {
    Router::new()
        .route("/api/v1/emails/{id}/reprocess", post(reprocess))
        .route("/api/v1/emails/{id}/review", post(review))
        .route("/api/v1/emails/{id}/approve", post(approve))
        .route("/api/v1/emails/{id}/reject", post(reject))
}
