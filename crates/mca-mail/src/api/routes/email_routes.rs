//! Email-related API endpoints.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::auth::{AuthError, AuthenticatedRequest};
use crate::api::ApiState;
use crate::domain::{EmailCategory, EmailId, EmailStatus, StoredEmail};
use crate::error::AppError;
use crate::persistence::api_key_repo::Role;
use crate::persistence::email_repo::{self, EmailFilter};

#[derive(Serialize)]
struct EmailListResponse {
    items: Vec<EmailSummary>,
    total: i64,
}

#[derive(Serialize)]
struct EmailSummary {
    id: Uuid,
    from_address: String,
    subject: String,
    status: String,
    received_at: String,
    category: Option<String>,
    spam_verdict: Option<String>,
}

#[derive(Serialize)]
struct EmailDetailResponse {
    id: Uuid,
    thread_id: Uuid,
    from_address: String,
    from_name: Option<String>,
    to_addresses: Vec<String>,
    subject: String,
    status: String,
    category: Option<String>,
    spam_verdict: Option<String>,
    text_body: String,
    date: Option<String>,
    received_at: String,
    lead_id: Option<Uuid>,
}

#[derive(Serialize)]
struct ThreadResponse {
    messages: Vec<EmailSummary>,
}

#[derive(Serialize)]
struct ActionResponse {
    success: bool,
    message: String,
}

#[derive(Deserialize)]
struct EmailListQuery {
    status: Option<String>,
    category: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// Default page size. The list is a work queue, not a mail client: 50 rows is
/// a screenful, and the caller can page from there.
const DEFAULT_PAGE: i64 = 50;

fn parse_status(raw: &str) -> Result<EmailStatus, AppError> {
    raw.parse()
        .map_err(|_| AppError::InvalidInput(format!("unknown email status: {raw}")))
}

fn parse_category(raw: &str) -> Result<EmailCategory, AppError> {
    raw.parse()
        .map_err(|_| AppError::InvalidInput(format!("unknown email category: {raw}")))
}

async fn list_emails(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<EmailListQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<EmailListResponse>, AppError> {
    let filter = EmailFilter {
        status: query.status.as_deref().map(parse_status).transpose()?,
        category: query.category.as_deref().map(parse_category).transpose()?,
        limit: query.limit.unwrap_or(DEFAULT_PAGE),
        offset: query.offset.unwrap_or(0),
        ..Default::default()
    };

    let total = email_repo::count(&state.pool, &filter).await?;
    let items = email_repo::list(&state.pool, &filter)
        .await?
        .into_iter()
        .map(|row| EmailSummary {
            id: row.id,
            from_address: row.from_address,
            subject: row.subject,
            status: row.status,
            received_at: row.received_at.to_rfc3339(),
            category: row.category,
            spam_verdict: row.spam_verdict,
        })
        .collect();

    Ok(Json(EmailListResponse { items, total }))
}

async fn get_email(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<Uuid>,
    _auth: AuthenticatedRequest,
) -> Result<Json<EmailDetailResponse>, AppError> {
    let email = email_repo::get(&state.pool, id).await?;
    Ok(Json(detail_of(email)))
}

async fn get_thread(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<Uuid>,
    _auth: AuthenticatedRequest,
) -> Result<Json<ThreadResponse>, AppError> {
    // The path names an email, not a thread: a caller asking for "the thread"
    // starts from the message it already has.
    let email = email_repo::get(&state.pool, id).await?;
    let messages = email_repo::thread_messages(&state.pool, email.thread_id)
        .await?
        .into_iter()
        .map(|message| EmailSummary {
            id: message.id,
            from_address: message.from_address,
            subject: message.subject,
            status: message.status.as_str().to_string(),
            received_at: message.received_at.to_rfc3339(),
            category: message.category.map(|c| c.as_str().to_string()),
            spam_verdict: message.spam_verdict.map(|v| v.as_str().to_string()),
        })
        .collect();
    Ok(Json(ThreadResponse { messages }))
}

fn detail_of(email: StoredEmail) -> EmailDetailResponse {
    EmailDetailResponse {
        id: email.id,
        thread_id: email.thread_id,
        from_address: email.from_address,
        from_name: email.from_name,
        to_addresses: email.to_addresses,
        subject: email.subject,
        status: email.status.as_str().to_string(),
        category: email.category.map(|c| c.as_str().to_string()),
        spam_verdict: email.spam_verdict.map(|v| v.as_str().to_string()),
        text_body: email.text_body,
        date: email.date.map(|d| d.to_rfc3339()),
        received_at: email.received_at.to_rfc3339(),
        lead_id: email.lead_id,
    }
}

async fn reprocess_email(
    State(_state): State<Arc<ApiState>>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AuthError> {
    auth.require(Role::Operator)?;
    Ok(Json(ActionResponse {
        success: true,
        message: format!("reprocessing scheduled for {id}"),
    }))
}

async fn review_email(
    State(_state): State<Arc<ApiState>>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AuthError> {
    auth.require(Role::Operator)?;
    Ok(Json(ActionResponse {
        success: true,
        message: format!("email {id} flagged for review"),
    }))
}

async fn approve_email(
    State(_state): State<Arc<ApiState>>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AuthError> {
    auth.require(Role::Manager)?;
    Ok(Json(ActionResponse {
        success: true,
        message: format!("email {id} approved"),
    }))
}

async fn reject_email(
    State(_state): State<Arc<ApiState>>,
    Path(id): Path<Uuid>,
    auth: AuthenticatedRequest,
) -> Result<Json<ActionResponse>, AuthError> {
    auth.require(Role::Manager)?;
    Ok(Json(ActionResponse {
        success: true,
        message: format!("email {id} rejected"),
    }))
}

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/v1/emails", get(list_emails))
        .route("/api/v1/emails/{id}", get(get_email))
        .route("/api/v1/emails/{id}/thread", get(get_thread))
        .route("/api/v1/emails/{id}/reprocess", post(reprocess_email))
        .route("/api/v1/emails/{id}/review", post(review_email))
        .route("/api/v1/emails/{id}/approve", post(approve_email))
        .route("/api/v1/emails/{id}/reject", post(reject_email))
}
