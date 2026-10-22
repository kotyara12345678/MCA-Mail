//! Pipeline switches: pause/resume fetch, processing, delivery, and the
//! global automation kill switch — all without a restart.

use axum::{
    extract::State,
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::api::auth::AuthenticatedRequest;
use crate::api::SharedState;
use crate::error::AppError;
use crate::persistence::{api_key_repo::Role, settings_repo};

/// The switches the panel exposes. `true` = the loop is *paused* for the
/// three pauses; `true` = *allowed* for `automation_enabled`. `email_mode` is
/// the agent's current autonomy level (what it is running under right now).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineFlags {
    pub poll_paused: bool,
    pub queue_paused: bool,
    pub outbox_paused: bool,
    pub automation_enabled: bool,
    pub email_mode: String,
}

/// Partial update: absent fields keep their current value.
#[derive(Debug, Default, Deserialize)]
pub struct PipelinePatch {
    pub poll_paused: Option<bool>,
    pub queue_paused: Option<bool>,
    pub outbox_paused: Option<bool>,
    pub automation_enabled: Option<bool>,
    /// New autonomy level: `dry_run`, `review` or `auto`. Anything else is a
    /// 400 — a typo must never silently land the system in a mode nobody
    /// meant.
    pub email_mode: Option<String>,
}

pub(super) async fn load_flags(state: &SharedState) -> Result<PipelineFlags, AppError> {
    let default_mode = state
        .config
        .as_ref()
        .map(|c| c.security.email_mode)
        .unwrap_or_default();
    Ok(PipelineFlags {
        poll_paused: settings_repo::is_paused(&state.pool, settings_repo::POLL_PAUSED).await,
        queue_paused: settings_repo::is_paused(&state.pool, settings_repo::QUEUE_PAUSED).await,
        outbox_paused: settings_repo::is_paused(&state.pool, settings_repo::OUTBOX_PAUSED).await,
        automation_enabled: settings_repo::automation_enabled(&state.pool).await?,
        email_mode: settings_repo::effective_email_mode(&state.pool, default_mode)
            .await
            .as_str()
            .to_string(),
    })
}

async fn get_flags(
    State(state): State<SharedState>,
    _auth: AuthenticatedRequest,
) -> Result<Json<PipelineFlags>, AppError> {
    Ok(Json(load_flags(&state).await?))
}

async fn patch_flags(
    State(state): State<SharedState>,
    auth: AuthenticatedRequest,
    Json(patch): Json<PipelinePatch>,
) -> Result<Json<PipelineFlags>, AppError> {
    auth.require(Role::Operator)?;
    let by = auth.api_key_prefix.clone();
    apply_bool(&state, settings_repo::POLL_PAUSED, patch.poll_paused, &by).await?;
    apply_bool(&state, settings_repo::QUEUE_PAUSED, patch.queue_paused, &by).await?;
    apply_bool(
        &state,
        settings_repo::OUTBOX_PAUSED,
        patch.outbox_paused,
        &by,
    )
    .await?;
    if let Some(enabled) = patch.automation_enabled {
        settings_repo::set_automation_enabled(&state.pool, enabled, &by).await?;
    }
    if let Some(raw_mode) = patch.email_mode {
        let mode = crate::config::EmailMode::parse(&raw_mode).ok_or_else(|| {
            AppError::InvalidInput(format!(
                "unknown email_mode {raw_mode:?}; expected dry_run, review or auto"
            ))
        })?;
        settings_repo::set_agent_mode(&state.pool, mode, &by).await?;
    }
    Ok(Json(load_flags(&state).await?))
}

/// Write one pause switch when the patch carries it.
async fn apply_bool(
    state: &SharedState,
    key: &str,
    value: Option<bool>,
    by: &str,
) -> Result<(), AppError> {
    if let Some(value) = value {
        settings_repo::set(&state.pool, key, serde_json::Value::Bool(value), by).await?;
    }
    Ok(())
}

pub fn routes() -> Router<SharedState> {
    Router::new().route(
        "/api/admin/settings",
        get(get_flags).merge(patch(patch_flags)),
    )
}
