use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    EmailId, LeadId, ProcessingRun, ProcessingStage, RunId, RunState, RunTrigger, ThreadId,
};
use crate::error::AppError;

use super::super::parse_enum;

#[derive(Debug, sqlx::FromRow)]
struct RunRow {
    id: Uuid,
    email_id: Uuid,
    thread_id: Uuid,
    lead_id: Option<Uuid>,
    state: String,
    trigger: String,
    attempt: i32,
    current_stage: Option<String>,
    stages_completed: Vec<String>,
    error: Option<String>,
    total_tokens: i64,
    cost_micros_usd: i64,
    started_at: chrono::DateTime<Utc>,
    finished_at: Option<chrono::DateTime<Utc>>,
}

const RUN_COLUMNS: &str = "id, email_id, thread_id, lead_id, state, trigger, attempt, \
     current_stage, stages_completed, error, total_tokens, cost_micros_usd, started_at, finished_at";

fn to_domain(row: RunRow) -> Result<ProcessingRun, AppError> {
    let stages = row
        .stages_completed
        .iter()
        .map(|s| parse_enum::<ProcessingStage>(s, "runs.stages_completed"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ProcessingRun {
        id: row.id,
        email_id: row.email_id,
        thread_id: row.thread_id,
        lead_id: row.lead_id,
        state: parse_enum(&row.state, "runs.state")?,
        trigger: parse_enum(&row.trigger, "runs.trigger")?,
        attempt: row.attempt,
        current_stage: row
            .current_stage
            .as_deref()
            .map(|s| parse_enum::<ProcessingStage>(s, "runs.current_stage"))
            .transpose()?,
        stages_completed: stages,
        error: row.error,
        total_tokens: row.total_tokens,
        cost_micros_usd: row.cost_micros_usd,
        started_at: row.started_at,
        finished_at: row.finished_at,
    })
}

/// Start a run, continuing the attempt counter from previous attempts.
///
/// A partial unique index permits only one live run per email, so a double click
/// on "reprocess" cannot start two concurrent pipelines.
pub async fn start(
    pool: &PgPool,
    email_id: EmailId,
    thread_id: ThreadId,
    trigger: RunTrigger,
) -> Result<ProcessingRun, AppError> {
    let previous = sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(max(attempt), 0) FROM email_processing_runs WHERE email_id = $1",
    )
    .bind(email_id)
    .fetch_one(pool)
    .await?;
    let row = sqlx::query_as::<_, RunRow>(&format!(
        "INSERT INTO email_processing_runs (email_id, thread_id, state, trigger, attempt) \
         VALUES ($1,$2,'running',$3,$4) RETURNING {RUN_COLUMNS}"
    ))
    .bind(email_id)
    .bind(thread_id)
    .bind(trigger.as_str())
    .bind(previous + 1)
    .fetch_one(pool)
    .await?;
    to_domain(row)
}

pub async fn get(pool: &PgPool, id: RunId) -> Result<Option<ProcessingRun>, AppError> {
    let row = sqlx::query_as::<_, RunRow>(&format!(
        "SELECT {RUN_COLUMNS} FROM email_processing_runs WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(to_domain).transpose()
}

/// Record stage progress so a crash is diagnosable and a resume can skip work.
pub async fn record_stage(
    pool: &PgPool,
    id: RunId,
    stage: ProcessingStage,
    completed: bool,
) -> Result<(), AppError> {
    let sql = if completed {
        "UPDATE email_processing_runs SET current_stage = $2, \
         stages_completed = array_append(stages_completed, $2) WHERE id = $1"
    } else {
        "UPDATE email_processing_runs SET current_stage = $2 WHERE id = $1"
    };
    sqlx::query(sql)
        .bind(id)
        .bind(stage.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn finish(
    pool: &PgPool,
    id: RunId,
    state: RunState,
    error: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE email_processing_runs SET state = $2, error = $3, finished_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(state.as_str())
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn add_cost(
    pool: &PgPool,
    id: RunId,
    tokens: i64,
    cost_micros: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE email_processing_runs SET total_tokens = total_tokens + $2, \
         cost_micros_usd = cost_micros_usd + $3 WHERE id = $1",
    )
    .bind(id)
    .bind(tokens)
    .bind(cost_micros)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_lead(pool: &PgPool, id: RunId, lead_id: LeadId) -> Result<(), AppError> {
    sqlx::query("UPDATE email_processing_runs SET lead_id = $2 WHERE id = $1")
        .bind(id)
        .bind(lead_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// A run that stopped mid-way and can be continued from its recorded stage.
pub async fn find_resumable(
    pool: &PgPool,
    email_id: EmailId,
) -> Result<Option<ProcessingRun>, AppError> {
    let row = sqlx::query_as::<_, RunRow>(&format!(
        "SELECT {RUN_COLUMNS} FROM email_processing_runs \
         WHERE email_id = $1 AND state IN ('failed','running') ORDER BY started_at DESC LIMIT 1"
    ))
    .bind(email_id)
    .fetch_optional(pool)
    .await?;
    row.map(to_domain).transpose()
}

#[derive(Debug, Clone, Default)]
pub struct RunFilter {
    pub email_id: Option<EmailId>,
    pub lead_id: Option<LeadId>,
    pub state: Option<RunState>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct RunSummary {
    pub id: Uuid,
    pub email_id: Uuid,
    pub lead_id: Option<Uuid>,
    pub state: String,
    pub trigger: String,
    pub attempt: i32,
    pub current_stage: Option<String>,
    pub total_tokens: i64,
    pub cost_micros_usd: i64,
    pub started_at: chrono::DateTime<Utc>,
    pub finished_at: Option<chrono::DateTime<Utc>>,
}

pub async fn list(pool: &PgPool, filter: &RunFilter) -> Result<Vec<RunSummary>, AppError> {
    let rows = sqlx::query_as::<_, RunSummary>(
        "SELECT id, email_id, lead_id, state, trigger, attempt, current_stage, total_tokens, \
         cost_micros_usd, started_at, finished_at FROM email_processing_runs \
         WHERE ($1::uuid IS NULL OR email_id = $1) AND ($2::uuid IS NULL OR lead_id = $2) \
         AND ($3::text IS NULL OR state = $3) \
         ORDER BY started_at DESC, id DESC LIMIT $4 OFFSET $5",
    )
    .bind(filter.email_id)
    .bind(filter.lead_id)
    .bind(filter.state.map(|s| s.as_str().to_string()))
    .bind(filter.limit.clamp(1, 500))
    .bind(filter.offset.max(0))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn count(pool: &PgPool, filter: &RunFilter) -> Result<i64, AppError> {
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_processing_runs \
         WHERE ($1::uuid IS NULL OR email_id = $1) AND ($2::uuid IS NULL OR lead_id = $2) \
         AND ($3::text IS NULL OR state = $3)",
    )
    .bind(filter.email_id)
    .bind(filter.lead_id)
    .bind(filter.state.map(|s| s.as_str().to_string()))
    .fetch_one(pool)
    .await?;
    Ok(total)
}
