use sqlx::PgPool;
use uuid::Uuid;

use super::row::{to_domain, LeadRow, COLUMNS};
use crate::domain::{Lead, LeadId, LeadStatus};
use crate::error::AppError;

/// Whitelisted lead filters. Absent filters bind as `NULL`.
#[derive(Debug, Clone, Default)]
pub struct LeadFilter {
    pub status: Option<LeadStatus>,
    pub contact_email: Option<String>,
    pub callback_only: bool,
    pub limit: i64,
    pub offset: i64,
}

/// Compact projection for list endpoints.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct LeadSummary {
    pub id: Uuid,
    pub status: String,
    pub category: Option<String>,
    pub scope: String,
    pub contact_email: String,
    pub company_name: Option<String>,
    pub company_inn: Option<String>,
    pub summary: Option<String>,
    pub confidence: Option<f32>,
    pub open_questions: serde_json::Value,
    pub callback_requested: bool,
    pub automation_locked: bool,
    pub last_activity_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const LIST_COLUMNS: &str = "id, status, category, scope, contact_email, company_name, \
     company_inn, summary, confidence, open_questions, callback_requested, automation_locked, \
     last_activity_at, created_at";

pub async fn list(pool: &PgPool, filter: &LeadFilter) -> Result<Vec<LeadSummary>, AppError> {
    let sql = format!(
        "SELECT {LIST_COLUMNS} FROM leads \
         WHERE ($1::text IS NULL OR status = $1) \
         AND ($2::text IS NULL OR contact_email = $2) \
         AND ($3::bool = FALSE OR callback_requested) \
         ORDER BY last_activity_at DESC, id DESC LIMIT $4 OFFSET $5"
    );
    let rows = sqlx::query_as::<_, LeadSummary>(&sql)
        .bind(filter.status.map(|s| s.as_str().to_string()))
        .bind(&filter.contact_email)
        .bind(filter.callback_only)
        .bind(filter.limit.clamp(1, 500))
        .bind(filter.offset.max(0))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn count(pool: &PgPool, filter: &LeadFilter) -> Result<i64, AppError> {
    let sql = "SELECT count(*) FROM leads \
               WHERE ($1::text IS NULL OR status = $1) \
               AND ($2::text IS NULL OR contact_email = $2) \
               AND ($3::bool = FALSE OR callback_requested)";
    let total: i64 = sqlx::query_scalar(sql)
        .bind(filter.status.map(|s| s.as_str().to_string()))
        .bind(&filter.contact_email)
        .bind(filter.callback_only)
        .fetch_one(pool)
        .await?;
    Ok(total)
}

pub async fn get(pool: &PgPool, id: LeadId) -> Result<Lead, AppError> {
    let sql = format!("SELECT {COLUMNS} FROM leads WHERE id = $1");
    let row = sqlx::query_as::<_, LeadRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("lead {id} not found")))?;
    to_domain(row)
}
