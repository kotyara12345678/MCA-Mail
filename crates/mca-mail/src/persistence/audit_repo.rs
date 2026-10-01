use chrono::Utc;
use sqlx::PgPool;

use crate::error::AppError;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct AuditRow {
    pub id: uuid::Uuid,
    pub actor: String,
    pub actor_role: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub outcome: String,
    pub details: serde_json::Value,
    pub request_id: Option<String>,
    pub remote_addr: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Denied,
    Failure,
}

impl Outcome {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::Denied => "denied",
            Outcome::Failure => "failure",
        }
    }
}

/// One audited action.
///
/// A struct rather than ten positional parameters: `actor`/`role`/`action` are
/// all `&str`, so a swapped pair would compile and silently misattribute an
/// administrative action.
#[derive(Debug, Clone)]
pub struct AuditEntry<'a> {
    pub actor: &'a str,
    pub actor_role: &'a str,
    pub action: &'a str,
    pub resource_type: &'a str,
    pub resource_id: Option<&'a str>,
    pub outcome: Outcome,
    pub details: serde_json::Value,
    pub request_id: Option<&'a str>,
    pub remote_addr: Option<&'a str>,
}

impl<'a> AuditEntry<'a> {
    pub fn new(
        actor: &'a str,
        actor_role: &'a str,
        action: &'a str,
        resource_type: &'a str,
        outcome: Outcome,
    ) -> Self {
        Self {
            actor,
            actor_role,
            action,
            resource_type,
            resource_id: None,
            outcome,
            details: serde_json::Value::Null,
            request_id: None,
            remote_addr: None,
        }
    }

    pub fn resource(mut self, id: &'a str) -> Self {
        self.resource_id = Some(id);
        self
    }

    pub fn details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }

    pub fn request(mut self, request_id: &'a str, remote_addr: &'a str) -> Self {
        self.request_id = Some(request_id);
        self.remote_addr = Some(remote_addr);
        self
    }
}

/// Append an audit entry. Called for every administrative and mutating action,
/// including the denied ones: a refusal is exactly what an auditor wants to see.
pub async fn record(pool: &PgPool, entry: &AuditEntry<'_>) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO audit_logs (actor, actor_role, action, resource_type, resource_id, \
         outcome, details, request_id, remote_addr) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(truncate(entry.actor))
    .bind(truncate(entry.actor_role))
    .bind(truncate(entry.action))
    .bind(truncate(entry.resource_type))
    .bind(entry.resource_id.map(truncate))
    .bind(entry.outcome.as_str())
    .bind(&entry.details)
    .bind(entry.request_id)
    .bind(entry.remote_addr)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub actor: Option<String>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub outcome: Option<Outcome>,
    pub since: Option<chrono::DateTime<Utc>>,
    pub limit: i64,
    pub offset: i64,
}

pub async fn list(pool: &PgPool, filter: &AuditFilter) -> Result<Vec<AuditRow>, AppError> {
    let rows = sqlx::query_as::<_, AuditRow>(
        "SELECT id, actor, actor_role, action, resource_type, resource_id, outcome, details, \
         request_id, remote_addr, created_at FROM audit_logs \
         WHERE ($1::text IS NULL OR actor = $1) AND ($2::text IS NULL OR action = $2) \
         AND ($3::text IS NULL OR resource_type = $3) \
         AND ($4::text IS NULL OR outcome = $4) \
         AND ($5::timestamptz IS NULL OR created_at >= $5) \
         ORDER BY created_at DESC, id DESC LIMIT $6 OFFSET $7",
    )
    .bind(&filter.actor)
    .bind(&filter.action)
    .bind(&filter.resource_type)
    .bind(filter.outcome.map(|o| o.as_str().to_string()))
    .bind(filter.since)
    .bind(filter.limit.clamp(1, 500))
    .bind(filter.offset.max(0))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn count(pool: &PgPool, filter: &AuditFilter) -> Result<i64, AppError> {
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs \
         WHERE ($1::text IS NULL OR actor = $1) AND ($2::text IS NULL OR action = $2) \
         AND ($3::text IS NULL OR resource_type = $3) \
         AND ($4::text IS NULL OR outcome = $4) \
         AND ($5::timestamptz IS NULL OR created_at >= $5)",
    )
    .bind(&filter.actor)
    .bind(&filter.action)
    .bind(&filter.resource_type)
    .bind(filter.outcome.map(|o| o.as_str().to_string()))
    .bind(filter.since)
    .fetch_one(pool)
    .await?;
    Ok(total)
}

pub async fn purge(pool: &PgPool, older_than_days: i32) -> Result<u64, AppError> {
    let done =
        sqlx::query("DELETE FROM audit_logs WHERE created_at < now() - make_interval(days => $1)")
            .bind(older_than_days)
            .execute(pool)
            .await?;
    Ok(done.rows_affected())
}

fn truncate(value: &str) -> String {
    if value.len() <= 256 {
        return value.to_string();
    }
    let mut end = 256;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
