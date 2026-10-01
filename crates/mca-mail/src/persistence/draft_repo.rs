use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{DraftId, DraftStatus, EmailDraft, EmailId, LeadId};
use crate::error::AppError;

#[derive(Debug, sqlx::FromRow)]
struct DraftRow {
    id: Uuid,
    lead_id: Option<Uuid>,
    email_id: Option<Uuid>,
    run_id: Option<Uuid>,
    in_reply_to: Option<String>,
    to_addresses: Vec<String>,
    cc_addresses: Vec<String>,
    subject: String,
    body: String,
    status: String,
    idempotency_key: String,
    suppression_reason: Option<String>,
    reviewed_by: Option<String>,
    reviewed_at: Option<chrono::DateTime<chrono::Utc>>,
    sent_at: Option<chrono::DateTime<chrono::Utc>>,
    provider_message_id: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

const COLUMNS: &str = "id, lead_id, email_id, run_id, in_reply_to, to_addresses, cc_addresses, \
     subject, body, status, idempotency_key, suppression_reason, reviewed_by, reviewed_at, \
     sent_at, provider_message_id, created_at";

fn to_domain(row: DraftRow) -> Result<EmailDraft, AppError> {
    Ok(EmailDraft {
        id: row.id,
        lead_id: row.lead_id,
        email_id: row.email_id,
        in_reply_to: row.in_reply_to,
        to_addresses: row.to_addresses,
        cc_addresses: row.cc_addresses,
        subject: row.subject,
        body: row.body,
        status: row
            .status
            .parse()
            .map_err(|e: crate::domain::WireParseError| sqlx::Error::Decode(Box::new(e)))?,
        idempotency_key: row.idempotency_key,
        suppression_reason: row.suppression_reason,
        reviewed_by: row.reviewed_by,
        reviewed_at: row.reviewed_at,
        sent_at: row.sent_at,
        provider_message_id: row.provider_message_id,
        created_at: row.created_at,
    })
}

/// Deterministic key for a generated reply.
///
/// Hashing run id, lead id and body means a reprocess of the same message
/// produces the same key, so the unique index on `idempotency_key` turns
/// at-most-once delivery into a database guarantee rather than a code path.
pub fn idempotency_key(run_id: Uuid, lead_id: Uuid, body: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(run_id.as_bytes());
    hasher.update(lead_id.as_bytes());
    hasher.update(b"|");
    hasher.update(body.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Insert a draft. Returns the existing row when the key was already used.
///
/// When the lead already has a live (pending/approved) draft, that draft is
/// returned instead of violating `uq_drafts_live_per_lead`: one pending reply
/// per lead is the business rule, not an error.
pub async fn create(pool: &PgPool, draft: &EmailDraft) -> Result<(DraftId, bool), AppError> {
    let existing =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM email_drafts WHERE idempotency_key = $1")
            .bind(&draft.idempotency_key)
            .fetch_optional(pool)
            .await?;
    if let Some(id) = existing {
        return Ok((id, false));
    }
    if let Some(lead_id) = draft.lead_id {
        if let Some(id) = live_draft_for_lead(pool, lead_id).await? {
            return Ok((id, false));
        }
    }
    let sql = "INSERT INTO email_drafts (lead_id, email_id, run_id, in_reply_to, to_addresses, \
         cc_addresses, subject, body, status, idempotency_key, suppression_reason) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
         ON CONFLICT (idempotency_key) DO UPDATE SET created_at = email_drafts.created_at \
         RETURNING id"
        .to_string();
    let insert = || {
        sqlx::query_scalar::<_, Uuid>(&sql)
            .bind(draft.lead_id)
            .bind(draft.email_id)
            .bind(None::<Uuid>)
            .bind(&draft.in_reply_to)
            .bind(&draft.to_addresses)
            .bind(&draft.cc_addresses)
            .bind(&draft.subject)
            .bind(&draft.body)
            .bind(draft.status.as_str())
            .bind(&draft.idempotency_key)
            .bind(&draft.suppression_reason)
            .fetch_one(pool)
    };
    match insert().await {
        Ok(id) => Ok((id, true)),
        // Lost a race against a concurrent insert of the lead's live draft.
        Err(sqlx::Error::Database(e)) if e.constraint() == Some("uq_drafts_live_per_lead") => {
            let lead_id = draft
                .lead_id
                .ok_or_else(|| AppError::Internal("draft without lead hit live index".into()))?;
            let id = live_draft_for_lead(pool, lead_id)
                .await?
                .ok_or_else(|| AppError::Internal("live draft vanished after conflict".into()))?;
            Ok((id, false))
        }
        Err(e) => Err(e.into()),
    }
}

pub async fn get(pool: &PgPool, id: DraftId) -> Result<EmailDraft, AppError> {
    let row =
        sqlx::query_as::<_, DraftRow>(&format!("SELECT {COLUMNS} FROM email_drafts WHERE id = $1"))
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("draft {id} not found")))?;
    to_domain(row)
}

pub async fn mark(
    pool: &PgPool,
    id: DraftId,
    status: DraftStatus,
    reviewed_by: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE email_drafts SET status = $2, reviewed_by = $3, reviewed_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status.as_str())
    .bind(reviewed_by)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_sent(
    pool: &PgPool,
    id: DraftId,
    provider_message_id: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE email_drafts SET status = 'sent', sent_at = now(), provider_message_id = $2 WHERE id = $1")
        .bind(id)
        .bind(provider_message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn suppress(pool: &PgPool, id: DraftId, reason: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE email_drafts SET status = 'suppressed', suppression_reason = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

/// A live draft already exists for this lead: a second reply would duplicate it.
pub async fn live_draft_for_lead(
    pool: &PgPool,
    lead_id: LeadId,
) -> Result<Option<DraftId>, AppError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM email_drafts WHERE lead_id = $1 \
         AND status IN ('pending_approval','approved') LIMIT 1",
    )
    .bind(lead_id)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

#[derive(Debug, Clone, Default)]
pub struct DraftFilter {
    pub lead_id: Option<LeadId>,
    pub email_id: Option<EmailId>,
    pub status: Option<DraftStatus>,
    pub limit: i64,
    pub offset: i64,
}

pub async fn list(pool: &PgPool, filter: &DraftFilter) -> Result<Vec<EmailDraft>, AppError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM email_drafts \
         WHERE ($1::uuid IS NULL OR lead_id = $1) AND ($2::uuid IS NULL OR email_id = $2) \
         AND ($3::text IS NULL OR status = $3) ORDER BY created_at DESC LIMIT $4 OFFSET $5"
    );
    let rows = sqlx::query_as::<_, DraftRow>(&sql)
        .bind(filter.lead_id)
        .bind(filter.email_id)
        .bind(filter.status.map(|s| s.as_str().to_string()))
        .bind(filter.limit.clamp(1, 200))
        .bind(filter.offset.max(0))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(to_domain).collect()
}

pub async fn pending_approval(pool: &PgPool, limit: i64) -> Result<Vec<EmailDraft>, AppError> {
    list(
        pool,
        &DraftFilter {
            status: Some(DraftStatus::PendingApproval),
            limit,
            ..Default::default()
        },
    )
    .await
}

/// Count of messages already sent to a recipient in the trailing window.
pub async fn sends_since(
    pool: &PgPool,
    address: &str,
    since: chrono::DateTime<chrono::Utc>,
) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_drafts \
         WHERE status = 'sent' AND sent_at >= $2 AND $1 = ANY(to_addresses)",
    )
    .bind(address)
    .bind(since)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Total sends in the trailing window, for the global rate limit.
pub async fn total_sends_since(
    pool: &PgPool,
    since: chrono::DateTime<chrono::Utc>,
) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_drafts WHERE status = 'sent' AND sent_at >= $1",
    )
    .bind(since)
    .fetch_one(pool)
    .await?;
    Ok(count)
}
