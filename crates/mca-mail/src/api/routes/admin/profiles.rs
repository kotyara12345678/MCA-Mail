//! LLM model profiles: list/create/edit/delete and switch the active one.
//!
//! The key never leaves this module in a response — list returns a prefix
//! only, and `activate` is the one call the provider refresh notices within
//! seconds.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use uuid::Uuid;

use crate::api::auth::AuthenticatedRequest;
use crate::api::SharedState;
use crate::error::AppError;
use crate::persistence::{
    api_key_repo::Role,
    llm_profile_repo::{self, NewProfile, ProfilePatch, ProfileSummary},
};

/// A pre-filled model option for the "add profile" form. Prices are the
/// published list prices — the operator can correct them before saving, and
/// the stored values are what cost accounting uses.
#[derive(Debug, Serialize)]
pub struct Preset {
    pub label: &'static str,
    pub base_url: &'static str,
    pub model: &'static str,
    pub price_prompt_per_million_usd: f64,
    pub price_completion_per_million_usd: f64,
}

const PRESETS: &[Preset] = &[
    Preset {
        label: "DeepSeek Chat (4 Flash)",
        base_url: "https://api.deepseek.com",
        model: "deepseek-chat",
        price_prompt_per_million_usd: 0.27,
        price_completion_per_million_usd: 1.10,
    },
    Preset {
        label: "DeepSeek Reasoner",
        base_url: "https://api.deepseek.com",
        model: "deepseek-reasoner",
        price_prompt_per_million_usd: 0.55,
        price_completion_per_million_usd: 2.19,
    },
    Preset {
        label: "Gemini 2.5 Flash",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai/",
        model: "gemini-2.5-flash",
        price_prompt_per_million_usd: 0.30,
        price_completion_per_million_usd: 2.50,
    },
];

async fn list_profiles(
    State(state): State<SharedState>,
    _auth: AuthenticatedRequest,
) -> Result<Json<Vec<ProfileSummary>>, AppError> {
    Ok(Json(llm_profile_repo::list(&state.pool).await?))
}

async fn create_profile(
    State(state): State<SharedState>,
    auth: AuthenticatedRequest,
    Json(new): Json<NewProfile>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), AppError> {
    auth.require(Role::Operator)?;
    let id = llm_profile_repo::create(&state.pool, &new).await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({"id": id})),
    ))
}

async fn patch_profile(
    State(state): State<SharedState>,
    auth: AuthenticatedRequest,
    Path(id): Path<Uuid>,
    Json(patch): Json<ProfilePatch>,
) -> Result<Json<serde_json::Value>, AppError> {
    auth.require(Role::Operator)?;
    let updated = llm_profile_repo::update(&state.pool, id, patch).await?;
    match updated {
        Some(_) => Ok(Json(serde_json::json!({"id": id, "updated": true}))),
        None => Err(AppError::NotFound(format!("llm profile {id} not found"))),
    }
}

async fn delete_profile(
    State(state): State<SharedState>,
    auth: AuthenticatedRequest,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    auth.require(Role::Operator)?;
    if llm_profile_repo::delete(&state.pool, id).await? {
        // The provider may still hold a snapshot of the deleted row; clearing
        // the selection here makes the next refresh unambiguous even if the
        // deleted row was the active one.
        Ok(Json(serde_json::json!({"id": id, "deleted": true})))
    } else {
        Err(AppError::NotFound(format!("llm profile {id} not found")))
    }
}

async fn activate_profile(
    State(state): State<SharedState>,
    auth: AuthenticatedRequest,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    auth.require(Role::Operator)?;
    llm_profile_repo::set_active(&state.pool, Some(id)).await?;
    Ok(Json(serde_json::json!({"active_id": id})))
}

async fn deactivate_profiles(
    State(state): State<SharedState>,
    auth: AuthenticatedRequest,
) -> Result<Json<serde_json::Value>, AppError> {
    auth.require(Role::Operator)?;
    llm_profile_repo::set_active(&state.pool, None).await?;
    Ok(Json(serde_json::json!({"active_id": null})))
}

async fn presets(
    State(_state): State<SharedState>,
    _auth: AuthenticatedRequest,
) -> Json<&'static [Preset]> {
    Json(PRESETS)
}

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route(
            "/api/admin/llm/profiles",
            get(list_profiles).post(create_profile),
        )
        .route(
            "/api/admin/llm/profiles/deactivate",
            post(deactivate_profiles),
        )
        .route(
            "/api/admin/llm/profiles/{id}",
            axum::routing::patch(patch_profile).delete(delete_profile),
        )
        .route(
            "/api/admin/llm/profiles/{id}/activate",
            post(activate_profile),
        )
        .route("/api/admin/llm/presets", get(presets))
}
