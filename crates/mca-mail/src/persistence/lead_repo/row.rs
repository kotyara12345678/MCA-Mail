use chrono::Utc;
use sqlx::FromRow;
use uuid::Uuid;

use super::super::parse_enum;
use crate::domain::{EmailCategory, Lead, LeadSource, LeadStatus, RequirementScope};
use crate::error::AppError;

#[derive(Debug, Clone, FromRow)]
pub struct LeadRow {
    pub id: Uuid,
    pub thread_id: Option<Uuid>,
    pub primary_email_id: Option<Uuid>,
    pub source: String,
    pub status: String,
    pub category: Option<String>,
    pub scope: String,
    pub conversation_key: String,
    pub contact_email: String,
    pub company_name: Option<String>,
    pub company_inn: Option<String>,
    pub summary: Option<String>,
    pub confidence: Option<f32>,
    pub open_questions: serde_json::Value,
    pub unresolved_topics: serde_json::Value,
    pub callback_requested: bool,
    pub callback_phone: Option<String>,
    pub automation_released_by: Option<Uuid>,
    pub last_activity_at: chrono::DateTime<Utc>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

pub const COLUMNS: &str = "id, thread_id, primary_email_id, source, status, category, scope, \
     conversation_key, contact_email, company_name, company_inn, summary, confidence, \
     open_questions, unresolved_topics, callback_requested, callback_phone, \
     automation_released_by, last_activity_at, created_at, updated_at";

fn string_list(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn to_domain(row: LeadRow) -> Result<Lead, AppError> {
    let category = row
        .category
        .as_deref()
        .map(|v| parse_enum::<EmailCategory>(v, "leads.category"))
        .transpose()?;
    Ok(Lead {
        id: row.id,
        thread_id: row.thread_id,
        primary_email_id: row.primary_email_id,
        source: parse_enum::<LeadSource>(&row.source, "leads.source")?,
        status: parse_enum::<LeadStatus>(&row.status, "leads.status")?,
        category,
        scope: parse_enum::<RequirementScope>(&row.scope, "leads.scope")?,
        conversation_key: row.conversation_key,
        contact_email: row.contact_email,
        company_name: row.company_name,
        company_inn: row.company_inn,
        summary: row.summary,
        confidence: row.confidence,
        open_questions: string_list(&row.open_questions),
        unresolved_topics: string_list(&row.unresolved_topics),
        callback_requested: row.callback_requested,
        callback_phone: row.callback_phone,
        automation_released_by: row.automation_released_by,
        last_activity_at: row.last_activity_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}
