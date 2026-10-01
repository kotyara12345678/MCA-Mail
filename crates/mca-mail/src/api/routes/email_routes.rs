//! Email-related API endpoints.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::auth::{AuthError, AuthenticatedRequest};
use crate::api::ApiState;
use crate::domain::{EmailId, EmailStatus, StoredEmail};
use crate::persistence::api_key_repo::Role;
use crate::persistence::email_repo;

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

async fn list_emails(
    State(_state): State<Arc<ApiState>>,
    _auth: AuthenticatedRequest,
) -> Json<EmailListResponse> {
    // Simplified listing — in production, add pagination and filtering
    let items = Vec::new(); // Placeholder for real query
    Json(EmailListResponse { items, total: 0 })
}

async fn get_email(
    State(_state): State<Arc<ApiState>>,
    Path(id): Path<Uuid>,
    _auth: AuthenticatedRequest,
) -> Json<serde_json::Value> {
    // Placeholder
    Json(serde_json::json!({"id": id, "status": "pending"}))
}

async fn get_thread(
    State(_state): State<Arc<ApiState>>,
    Path(_id): Path<Uuid>,
    _auth: AuthenticatedRequest,
) -> Json<ThreadResponse> {
    Json(ThreadResponse { messages: vec![] })
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
