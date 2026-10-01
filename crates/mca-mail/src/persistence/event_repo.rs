use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    AgentKind, EmailId, EventSeverity, LeadId, ProcessingEvent, ProcessingStage, RunId, ToolCallId,
    ToolCallRecord, ToolCallStatus,
};
use crate::error::AppError;

/// Append an event. Never carries a message body: `details` holds identifiers
/// and counters only, which is what keeps the event stream safe to log and to
/// expose through the API.
///
/// A builder rather than nine positional arguments: the subject fields are all
/// optional, so positional order would be easy to get silently wrong.
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub run_id: Option<RunId>,
    pub email_id: Option<EmailId>,
    pub lead_id: Option<LeadId>,
    pub stage: Option<ProcessingStage>,
    pub severity: EventSeverity,
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

impl NewEvent {
    pub fn new(severity: EventSeverity, code: &str, message: &str) -> Self {
        Self {
            run_id: None,
            email_id: None,
            lead_id: None,
            stage: None,
            severity,
            code: code.to_string(),
            message: message.to_string(),
            details: serde_json::Value::Null,
        }
    }

    pub fn run(mut self, run_id: RunId) -> Self {
        self.run_id = Some(run_id);
        self
    }

    pub fn email(mut self, email_id: EmailId) -> Self {
        self.email_id = Some(email_id);
        self
    }

    pub fn lead(mut self, lead_id: LeadId) -> Self {
        self.lead_id = Some(lead_id);
        self
    }

    pub fn stage(mut self, stage: ProcessingStage) -> Self {
        self.stage = Some(stage);
        self
    }

    pub fn details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }
}

pub async fn event(pool: &PgPool, new: &NewEvent) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO processing_events (run_id, email_id, lead_id, stage, severity, code, \
         message, details) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(new.run_id)
    .bind(new.email_id)
    .bind(new.lead_id)
    .bind(new.stage.map(|s| s.as_str().to_string()))
    .bind(new.severity.as_str())
    .bind(&new.code)
    .bind(&new.message)
    .bind(&new.details)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub run_id: Option<RunId>,
    pub email_id: Option<EmailId>,
    pub lead_id: Option<LeadId>,
    pub severity: Option<EventSeverity>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct EventRow {
    pub id: Uuid,
    pub run_id: Option<Uuid>,
    pub email_id: Option<Uuid>,
    pub lead_id: Option<Uuid>,
    pub stage: Option<String>,
    pub severity: String,
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const PREDICATE: &str = "WHERE ($1::uuid IS NULL OR run_id = $1) \
     AND ($2::uuid IS NULL OR email_id = $2) \
     AND ($3::uuid IS NULL OR lead_id = $3) \
     AND ($4::text IS NULL OR severity = $4)";

pub async fn list(pool: &PgPool, filter: &EventFilter) -> Result<Vec<EventRow>, AppError> {
    let sql = format!(
        "SELECT id, run_id, email_id, lead_id, stage, severity, code, message, details, \
         created_at FROM processing_events {PREDICATE} ORDER BY created_at DESC, id DESC \
         LIMIT $5 OFFSET $6"
    );
    let rows = sqlx::query_as::<_, EventRow>(&sql)
        .bind(filter.run_id)
        .bind(filter.email_id)
        .bind(filter.lead_id)
        .bind(filter.severity.map(|s| s.as_str().to_string()))
        .bind(filter.limit.clamp(1, 500))
        .bind(filter.offset.max(0))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn count(pool: &PgPool, filter: &EventFilter) -> Result<i64, AppError> {
    let sql = format!("SELECT count(*) FROM processing_events {PREDICATE}");
    let total: i64 = sqlx::query_scalar(&sql)
        .bind(filter.run_id)
        .bind(filter.email_id)
        .bind(filter.lead_id)
        .bind(filter.severity.map(|s| s.as_str().to_string()))
        .fetch_one(pool)
        .await?;
    Ok(total)
}

pub async fn to_domain(row: EventRow) -> Result<ProcessingEvent, AppError> {
    Ok(ProcessingEvent {
        id: row.id,
        run_id: row.run_id,
        email_id: row.email_id,
        lead_id: row.lead_id,
        stage: row
            .stage
            .as_deref()
            .map(|s| s.parse::<ProcessingStage>())
            .transpose()
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        severity: row
            .severity
            .parse::<EventSeverity>()
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        code: row.code,
        message: row.message,
        details: row.details,
        created_at: row.created_at,
    })
}

// --- tool calls -----------------------------------------------------------

/// Record a tool invocation.
///
/// Arguments are redacted by the tool registry before they reach this function,
/// so a credential can never reach this table.
pub async fn tool_call(pool: &PgPool, record: &ToolCallRecord) -> Result<ToolCallId, AppError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO tool_calls (run_id, agent_run_id, email_id, agent, tool_name, arguments, \
         status, result_preview, error, duration_ms) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id",
    )
    .bind(record.run_id)
    .bind(record.agent_run_id)
    .bind(record.email_id)
    .bind(record.agent.as_str())
    .bind(&record.tool_name)
    .bind(&record.arguments)
    .bind(record.status.as_str())
    .bind(&record.result_preview)
    .bind(&record.error)
    .bind(record.duration_ms)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn update_tool_call(
    pool: &PgPool,
    id: ToolCallId,
    status: ToolCallStatus,
    result_preview: Option<&str>,
    error: Option<&str>,
    duration_ms: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE tool_calls SET status = $2, result_preview = $3, error = $4, duration_ms = $5 \
         WHERE id = $1",
    )
    .bind(id)
    .bind(status.as_str())
    .bind(result_preview)
    .bind(error)
    .bind(duration_ms)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct ToolCallRow {
    pub id: Uuid,
    pub run_id: Uuid,
    pub agent_run_id: Uuid,
    pub email_id: Uuid,
    pub agent: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub status: String,
    pub result_preview: Option<String>,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn tool_calls_for_run(
    pool: &PgPool,
    run_id: RunId,
) -> Result<Vec<ToolCallRow>, AppError> {
    let rows = sqlx::query_as::<_, ToolCallRow>(
        "SELECT id, run_id, agent_run_id, email_id, agent, tool_name, arguments, status, \
         result_preview, error, duration_ms, created_at FROM tool_calls WHERE run_id = $1 \
         ORDER BY created_at, id",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn to_tool_domain(row: ToolCallRow) -> Result<ToolCallRecord, AppError> {
    let agent: AgentKind = row
        .agent
        .parse()
        .map_err(|e: crate::domain::WireParseError| sqlx::Error::Decode(Box::new(e)))?;
    let status: ToolCallStatus = row
        .status
        .parse()
        .map_err(|e: crate::domain::WireParseError| sqlx::Error::Decode(Box::new(e)))?;
    Ok(ToolCallRecord {
        id: row.id,
        run_id: row.run_id,
        agent_run_id: row.agent_run_id,
        email_id: row.email_id,
        agent,
        tool_name: row.tool_name,
        arguments: row.arguments,
        status,
        result_preview: row.result_preview,
        error: row.error,
        duration_ms: row.duration_ms,
        created_at: row.created_at,
    })
}

/// Purge old operational events. Part of the retention job.
pub async fn purge_events(pool: &PgPool, older_than_days: i32) -> Result<u64, AppError> {
    let done = sqlx::query(
        "DELETE FROM processing_events \
         WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(older_than_days)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}
