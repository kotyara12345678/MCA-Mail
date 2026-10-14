//! Dashboard summary: the numbers and switches on the panel's front page.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::api::auth::AuthenticatedRequest;
use crate::api::SharedState;
use crate::error::AppError;
use crate::persistence::{
    draft_repo, email_repo, lead_repo, llm_profile_repo, outbox_repo, pool, run_repo, settings_repo,
};

#[derive(Debug, Serialize)]
pub struct SummaryView {
    pub uptime_seconds: u64,
    pub db_ok: bool,
    pub emails: EmailCounts,
    pub leads_total: i64,
    pub drafts_pending: i64,
    pub outbox_pending: i64,
    pub pipeline: super::settings::PipelineFlags,
    pub llm: LlmView,
    pub cost_today_micros: i64,
    pub cost_total_micros: i64,
}

#[derive(Debug, Serialize)]
pub struct EmailCounts {
    pub total: i64,
    /// Red-flagged rows, counted separately: `by_status` would hide them.
    pub flagged: i64,
    pub by_status: BTreeMap<String, i64>,
}

#[derive(Debug, Serialize)]
pub struct LlmView {
    /// Transport from configuration (`openai_compatible` / `mock`).
    pub provider: String,
    /// Model actually used right now: profile model when one is active,
    /// otherwise the configured default.
    pub active_model: String,
    /// Configured default, for comparison.
    pub config_model: String,
    pub profile: Option<ProfileRef>,
}

/// The active profile as the dashboard shows it — never the API key.
#[derive(Debug, Serialize)]
pub struct ProfileRef {
    pub id: uuid::Uuid,
    pub name: String,
    pub model: String,
}

/// Midnight UTC of the same day — the window `cost_since` needs.
fn start_of_today(now: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    use chrono::TimeZone;
    match now.date_naive().and_hms_opt(0, 0, 0) {
        Some(midnight) => chrono::Utc.from_utc_datetime(&midnight),
        None => now,
    }
}

async fn email_counts(state: &SharedState) -> Result<EmailCounts, AppError> {
    let rows = email_repo::status_counts(&state.pool).await?;
    let mut by_status = BTreeMap::new();
    let mut total = 0;
    for (status, count) in rows {
        total += count;
        by_status.insert(status, count);
    }
    let flagged = email_repo::count(
        &state.pool,
        &email_repo::EmailFilter {
            flagged: Some(true),
            limit: 1,
            ..Default::default()
        },
    )
    .await?;
    Ok(EmailCounts {
        total,
        flagged,
        by_status,
    })
}

async fn llm_view(state: &SharedState) -> Result<LlmView, AppError> {
    let (provider, config_model) = state
        .config
        .as_ref()
        .map(|cfg| (cfg.llm.provider.as_str().to_string(), cfg.llm.model.clone()))
        .unwrap_or_else(|| ("unknown".into(), String::new()));

    let profile = llm_profile_repo::active(&state.pool)
        .await?
        .map(|row| ProfileRef {
            id: row.id,
            name: row.name,
            model: row.model.clone(),
        });
    let active_model = profile
        .as_ref()
        .map(|p| p.model.clone())
        .unwrap_or_else(|| config_model.clone());
    Ok(LlmView {
        provider,
        active_model,
        config_model,
        profile,
    })
}

async fn summary(
    State(state): State<SharedState>,
    _auth: AuthenticatedRequest,
) -> Result<Json<SummaryView>, AppError> {
    let emails = email_counts(&state).await?;
    let leads_total = lead_repo::count(&state.pool, &Default::default()).await?;
    let drafts_pending = draft_repo::pending_approval_count(&state.pool).await?;
    let outbox_pending = outbox_repo::pending_count(&state.pool).await?;
    let pipeline = super::settings::load_flags(&state).await?;
    let llm = llm_view(&state).await?;
    let now = chrono::Utc::now();
    let cost_today_micros = run_repo::cost_since(&state.pool, start_of_today(now)).await?;
    let cost_total_micros = run_repo::total_cost(&state.pool).await?;

    let db_ok = matches!(
        tokio::time::timeout(Duration::from_secs(3), pool::health(&state.pool)).await,
        Ok(health) if health.reachable
    );

    Ok(Json(SummaryView {
        uptime_seconds: state.start_time.elapsed().as_secs(),
        db_ok,
        emails,
        leads_total,
        drafts_pending,
        outbox_pending,
        pipeline,
        llm,
        cost_today_micros,
        cost_total_micros,
    }))
}

pub fn routes() -> Router<SharedState> {
    Router::new().route("/api/admin/summary", get(summary))
}
