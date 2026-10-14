//! Token usage and cost breakdowns for the panel's cost page.

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::api::auth::AuthenticatedRequest;
use crate::api::SharedState;
use crate::error::AppError;
use crate::persistence::run_repo;

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    /// Window length in days, default 30.
    days: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct UsageView {
    pub days: i32,
    pub total: UsageTotals,
    pub by_day: Vec<run_repo::DayUsage>,
    pub by_model: Vec<run_repo::ModelUsage>,
    pub by_agent: Vec<run_repo::AgentStats>,
}

#[derive(Debug, Default, Serialize)]
pub struct UsageTotals {
    pub calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_micros_usd: i64,
}

/// Fold the daily rows into one window total — the same rows the chart draws,
/// so the headline number always matches the sum beneath it.
fn totals_over(days: &[run_repo::DayUsage]) -> UsageTotals {
    let mut totals = UsageTotals::default();
    for day in days {
        totals.calls += day.calls;
        totals.prompt_tokens += day.prompt_tokens;
        totals.completion_tokens += day.completion_tokens;
        totals.cost_micros_usd += day.cost_micros_usd;
    }
    totals
}

async fn usage(
    State(state): State<SharedState>,
    Query(query): Query<UsageQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<UsageView>, AppError> {
    let days = query.days.unwrap_or(30);
    let by_day = run_repo::usage_by_day(&state.pool, days).await?;
    let total = totals_over(&by_day);
    let by_model = run_repo::usage_by_model(&state.pool, days).await?;
    let by_agent = run_repo::agent_statistics(&state.pool).await?;
    Ok(Json(UsageView {
        days,
        total,
        by_day,
        by_model,
        by_agent,
    }))
}

pub fn routes() -> Router<SharedState> {
    Router::new().route("/api/admin/usage", get(usage))
}
