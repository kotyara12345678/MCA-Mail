use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{AgentKind, AgentRunId, AgentRunRecord, EmailId, RunId, RunState};
use crate::error::AppError;

use super::super::parse_enum;

#[derive(Debug, sqlx::FromRow)]
struct AgentRunRow {
    id: Uuid,
    run_id: Uuid,
    email_id: Uuid,
    agent: String,
    model: String,
    tier: String,
    state: String,
    iterations: i32,
    tool_calls: i32,
    prompt_tokens: i64,
    completion_tokens: i64,
    cost_micros_usd: i64,
    duration_ms: i64,
    error: Option<String>,
    started_at: chrono::DateTime<Utc>,
    finished_at: Option<chrono::DateTime<Utc>>,
}

const AGENT_COLUMNS: &str = "id, run_id, email_id, agent, model, tier, state, iterations, \
     tool_calls, prompt_tokens, completion_tokens, cost_micros_usd, duration_ms, error, \
     started_at, finished_at";

fn to_domain(row: AgentRunRow) -> Result<AgentRunRecord, AppError> {
    Ok(AgentRunRecord {
        id: row.id,
        run_id: row.run_id,
        email_id: row.email_id,
        agent: parse_enum(&row.agent, "agent_runs.agent")?,
        model: row.model,
        state: parse_enum(&row.state, "agent_runs.state")?,
        iterations: row.iterations,
        tool_calls: row.tool_calls,
        prompt_tokens: row.prompt_tokens,
        completion_tokens: row.completion_tokens,
        cost_micros_usd: row.cost_micros_usd,
        duration_ms: row.duration_ms,
        error: row.error,
        started_at: row.started_at,
        finished_at: row.finished_at,
    })
}

pub async fn start_agent(
    pool: &PgPool,
    run_id: RunId,
    email_id: EmailId,
    agent: AgentKind,
    model: &str,
    tier: &str,
) -> Result<AgentRunId, AppError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO agent_runs (run_id, email_id, agent, model, tier, state) \
         VALUES ($1,$2,$3,$4,$5,'running') RETURNING id",
    )
    .bind(run_id)
    .bind(email_id)
    .bind(agent.as_str())
    .bind(model)
    .bind(tier)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn finish_agent(
    pool: &PgPool,
    id: AgentRunId,
    state: RunState,
    iterations: i32,
    tool_calls: i32,
    duration_ms: i64,
    error: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE agent_runs SET state = $2, iterations = $3, tool_calls = $4, duration_ms = $5, \
         error = $6, finished_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(state.as_str())
    .bind(iterations)
    .bind(tool_calls)
    .bind(duration_ms)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn add_agent_cost(
    pool: &PgPool,
    id: AgentRunId,
    prompt_tokens: i64,
    completion_tokens: i64,
    cost_micros: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE agent_runs SET prompt_tokens = prompt_tokens + $2, \
         completion_tokens = completion_tokens + $3, cost_micros_usd = cost_micros_usd + $4 \
         WHERE id = $1",
    )
    .bind(id)
    .bind(prompt_tokens)
    .bind(completion_tokens)
    .bind(cost_micros)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct AgentRunSummary {
    pub id: Uuid,
    pub run_id: Uuid,
    pub email_id: Uuid,
    pub agent: String,
    pub model: String,
    pub state: String,
    pub iterations: i32,
    pub tool_calls: i32,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_micros_usd: i64,
    pub duration_ms: i64,
    pub error: Option<String>,
    pub started_at: chrono::DateTime<Utc>,
    pub finished_at: Option<chrono::DateTime<Utc>>,
}

pub async fn agent_runs(
    pool: &PgPool,
    run_id: Option<RunId>,
    limit: i64,
) -> Result<Vec<AgentRunSummary>, AppError> {
    let rows = sqlx::query_as::<_, AgentRunSummary>(
        "SELECT id, run_id, email_id, agent, model, state, iterations, tool_calls, \
         prompt_tokens, completion_tokens, cost_micros_usd, duration_ms, error, started_at, \
         finished_at FROM agent_runs WHERE ($1::uuid IS NULL OR run_id = $1) \
         ORDER BY started_at, id LIMIT $2",
    )
    .bind(run_id)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn agent_run(pool: &PgPool, id: AgentRunId) -> Result<Option<AgentRunRecord>, AppError> {
    let row = sqlx::query_as::<_, AgentRunRow>(&format!(
        "SELECT {AGENT_COLUMNS} FROM agent_runs WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(to_domain).transpose()
}

/// Success rate and cost per agent, backing `/api/v1/agents/status`.
pub async fn agent_statistics(pool: &PgPool) -> Result<Vec<AgentStats>, AppError> {
    let rows = sqlx::query_as::<_, AgentStats>(
        "SELECT agent, \
         count(*) AS total, \
         count(*) FILTER (WHERE state = 'succeeded') AS succeeded, \
         count(*) FILTER (WHERE state = 'failed') AS failed, \
         COALESCE(sum(prompt_tokens + completion_tokens), 0) AS tokens, \
         COALESCE(sum(cost_micros_usd), 0) AS cost_micros_usd, \
         COALESCE(avg(duration_ms), 0)::bigint AS avg_duration_ms \
         FROM agent_runs GROUP BY agent ORDER BY agent",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct AgentStats {
    pub agent: String,
    pub total: i64,
    pub succeeded: i64,
    pub failed: i64,
    pub tokens: i64,
    pub cost_micros_usd: i64,
    pub avg_duration_ms: i64,
}

/// Cost incurred since a point in time, used by the budget guard.
pub async fn cost_since(pool: &PgPool, since: chrono::DateTime<Utc>) -> Result<i64, AppError> {
    let total: Option<i64> =
        sqlx::query_scalar("SELECT sum(cost_micros_usd) FROM agent_runs WHERE started_at >= $1")
            .bind(since)
            .fetch_one(pool)
            .await?;
    Ok(total.unwrap_or(0))
}

/// Confirm that a lead is still inside its cost budget before an LLM call.
pub async fn total_cost(pool: &PgPool) -> Result<i64, AppError> {
    let total: Option<i64> = sqlx::query_scalar("SELECT sum(cost_micros_usd) FROM agent_runs")
        .fetch_one(pool)
        .await?;
    Ok(total.unwrap_or(0))
}
