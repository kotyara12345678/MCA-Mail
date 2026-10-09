use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{LeadId, LeadSource, LeadStatus, RequirementScope, ThreadId};
use crate::error::AppError;

/// Create a lead for a conversation, or return the existing one.
///
/// The unique index on `conversation_key` is the real guarantee: two workers
/// classifying the same message concurrently still end up with a single lead.
pub async fn ensure(
    pool: &PgPool,
    conversation_key: &str,
    contact_email: &str,
    thread_id: Option<ThreadId>,
    primary_email_id: Option<Uuid>,
    scope: RequirementScope,
) -> Result<(LeadId, bool), AppError> {
    if let Some(id) = find_by_key(pool, conversation_key).await? {
        return Ok((id, false));
    }
    let sql = "INSERT INTO leads (thread_id, primary_email_id, source, status, scope, \
         conversation_key, contact_email) VALUES ($1,$2,'inbound_email','new',$3,$4,$5) \
         ON CONFLICT (conversation_key) DO UPDATE SET last_activity_at = now() RETURNING id"
        .to_string();
    let id = sqlx::query_scalar::<_, Uuid>(&sql)
        .bind(thread_id)
        .bind(primary_email_id)
        .bind(scope.as_str())
        .bind(conversation_key)
        .bind(contact_email)
        .fetch_one(pool)
        .await?;
    Ok((id, true))
}

/// Manual creation through the admin API, keyed so a repeated call is a no-op.
pub async fn create_manual(
    pool: &PgPool,
    contact_email: &str,
    company_name: Option<&str>,
    company_inn: Option<&str>,
) -> Result<LeadId, AppError> {
    let key = format!("manual:{}", contact_email.to_ascii_lowercase());
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO leads (source, status, scope, conversation_key, contact_email, company_name, \
         company_inn) VALUES ('api','new','full_import',$1,$2,$3,$4) \
         ON CONFLICT (conversation_key) DO UPDATE SET updated_at = now() RETURNING id",
    )
    .bind(&key)
    .bind(contact_email)
    .bind(company_name)
    .bind(company_inn)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn find_by_key(pool: &PgPool, key: &str) -> Result<Option<LeadId>, AppError> {
    Ok(
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM leads WHERE conversation_key = $1")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}

/// An open lead for the same company, used to avoid a second opportunity when
/// the INN is already known.
pub async fn find_by_inn(pool: &PgPool, inn: &str) -> Result<Option<LeadId>, AppError> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM leads WHERE company_inn = $1 AND status NOT IN ('closed','lost') \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(inn)
    .fetch_optional(pool)
    .await?)
}

pub async fn source_of(pool: &PgPool, id: LeadId) -> Result<Option<LeadSource>, AppError> {
    let value = sqlx::query_scalar::<_, String>("SELECT source FROM leads WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    value
        .as_deref()
        .map(|v| super::super::parse_enum::<LeadSource>(v, "leads.source"))
        .transpose()
        .map_err(AppError::from)
}

pub async fn update_status(pool: &PgPool, id: LeadId, status: LeadStatus) -> Result<(), AppError> {
    sqlx::query("UPDATE leads SET status = $2 WHERE id = $1")
        .bind(id)
        .bind(status.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_summary(
    pool: &PgPool,
    id: LeadId,
    summary: &str,
    questions: &[String],
    unresolved: &[String],
    confidence: f32,
) -> Result<(), AppError> {
    // Both columns are `jsonb`; binding `&[String]` would be sent as `text[]`
    // and PostgreSQL would reject the assignment instead of coercing it.
    sqlx::query(
        "UPDATE leads SET summary = $2, open_questions = $3, unresolved_topics = $4, \
         confidence = $5, last_activity_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(summary)
    .bind(serde_json::Value::Array(
        questions
            .iter()
            .map(|q| serde_json::Value::String(q.clone()))
            .collect(),
    ))
    .bind(serde_json::Value::Array(
        unresolved
            .iter()
            .map(|q| serde_json::Value::String(q.clone()))
            .collect(),
    ))
    .bind(confidence)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_identity(
    pool: &PgPool,
    id: LeadId,
    company_name: Option<&str>,
    company_inn: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE leads SET company_name = COALESCE($2, company_name), \
         company_inn = COALESCE($3, company_inn) WHERE id = $1",
    )
    .bind(id)
    .bind(company_name)
    .bind(company_inn)
    .execute(pool)
    .await?;
    Ok(())
}

/// Store the customer's email address on the lead.
///
/// The voice channel starts with `contact_email` holding the caller number,
/// so this is where a dictated address replaces it — and with it, whether an
/// order card can be addressed at all. The value is validated by
/// `voice::normalize_email` before it reaches here; the caller owns the
/// format decision, this function owns persistence.
pub async fn set_contact_email(pool: &PgPool, id: LeadId, email: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE leads SET contact_email = $2, last_activity_at = now() WHERE id = $1")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_callback(
    pool: &PgPool,
    id: LeadId,
    requested: bool,
    phone: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE leads SET callback_requested = $2, \
         callback_phone = COALESCE($3, callback_phone) WHERE id = $1",
    )
    .bind(id)
    .bind(requested)
    .bind(phone)
    .execute(pool)
    .await?;
    Ok(())
}

/// Lock automation for a lead. Called whenever a handoff is created.
pub async fn lock_automation(pool: &PgPool, id: LeadId) -> Result<(), AppError> {
    sqlx::query("UPDATE leads SET automation_locked = TRUE, status = 'handed_off' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Release automation. Reachable only from an authenticated manager action.
pub async fn release_automation(
    pool: &PgPool,
    id: LeadId,
    released_by: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE leads SET automation_locked = FALSE, automation_released_by = $2, \
         status = 'awaiting_customer' WHERE id = $1",
    )
    .bind(id)
    .bind(released_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Defaults to locked when the lead cannot be found, so a missing row can never
/// accidentally re-enable autonomous sending.
pub async fn is_automation_locked(pool: &PgPool, id: LeadId) -> Result<bool, AppError> {
    let locked: Option<bool> =
        sqlx::query_scalar("SELECT automation_locked FROM leads WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(locked.unwrap_or(true))
}
