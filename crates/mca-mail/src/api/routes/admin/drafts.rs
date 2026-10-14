//! Reply drafts awaiting a human decision: the list, plus approve/reject.
//!
//! A draft only ever moves out of `pending_approval` — approving a draft that
//! was already sent (auto-send flips it on delivery) is a conflict, not a
//! silent overwrite.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::auth::AuthenticatedRequest;
use crate::api::routes::email_routes::ActionResponse;
use crate::api::SharedState;
use crate::domain::DraftStatus;
use crate::error::AppError;
use crate::persistence::{api_key_repo::Role, draft_repo};

use super::ops::{page, PageQuery, Paged};

#[derive(Debug, Deserialize)]
pub struct DraftQuery {
    /// `pending_approval` (default), `approved`, `rejected`, `sent`,
    /// `suppressed`, or empty for every status.
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

fn parse_status(raw: Option<&str>) -> Result<Option<DraftStatus>, AppError> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(value) => value
            .parse()
            .map(Some)
            .map_err(|e: crate::domain::WireParseError| AppError::InvalidInput(e.to_string())),
    }
}

async fn list(
    State(state): State<SharedState>,
    Query(query): Query<DraftQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<Paged<crate::domain::EmailDraft>>, AppError> {
    let (limit, offset) = page(&PageQuery {
        limit: query.limit,
        offset: query.offset,
    });
    let filter = draft_repo::DraftFilter {
        status: parse_status(query.status.as_deref())?,
        limit,
        offset,
        ..Default::default()
    };
    let total = draft_repo::count(&state.pool, &filter).await?;
    let items = draft_repo::list(&state.pool, &filter).await?;
    Ok(Json(Paged { items, total }))
}

/// Shared body of approve/reject: must exist, must still be pending.
async fn decide(
    state: &SharedState,
    id: Uuid,
    status: DraftStatus,
    by: &str,
) -> Result<Json<ActionResponse>, AppError> {
    let draft = draft_repo::get(&state.pool, id).await?;
    if draft.status != DraftStatus::PendingApproval {
        return Err(AppError::Conflict(format!(
            "draft is {}, not pending approval",
            draft.status.as_str()
        )));
    }
    draft_repo::mark(&state.pool, id, status, Some(by)).await?;
    let message = match status {
        DraftStatus::Approved => format!("draft {id} approved"),
        _ => format!("draft {id} rejected"),
    };
    Ok(Json(ActionResponse {
        success: true,
        message,
    }))
}

async fn approve(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AppError> {
    auth.require(Role::Manager)?;
    decide(&state, id, DraftStatus::Approved, &auth.api_key_prefix).await
}

async fn reject(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AppError> {
    auth.require(Role::Manager)?;
    decide(&state, id, DraftStatus::Rejected, &auth.api_key_prefix).await
}

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/api/admin/drafts", get(list))
        .route("/api/admin/drafts/{id}/approve", post(approve))
        .route("/api/admin/drafts/{id}/reject", post(reject))
}
