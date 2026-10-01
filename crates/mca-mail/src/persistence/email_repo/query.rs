use sqlx::PgPool;
use uuid::Uuid;

use super::super::rows::{EmailRow, EmailSummaryRow, EMAIL_COLUMNS};
use super::super::{parse_enum, parse_enum_opt};
use crate::domain::{
    EmailAttachment, EmailCategory, EmailId, EmailStatus, ExtractionStatus, InboundMessage,
    StoredEmail, ThreadId,
};
use crate::error::AppError;

/// Whitelisted list filters. Absent filters bind as `NULL`, so the SQL text is
/// constant and no user input is ever concatenated into a statement.
#[derive(Debug, Clone, Default)]
pub struct EmailFilter<'a> {
    pub status: Option<EmailStatus>,
    pub category: Option<EmailCategory>,
    pub from_address: Option<&'a str>,
    pub lead_id: Option<Uuid>,
    pub limit: i64,
    pub offset: i64,
}

impl EmailFilter<'_> {
    fn status_text(&self) -> Option<String> {
        self.status.map(|s| s.as_str().to_string())
    }

    fn category_text(&self) -> Option<String> {
        self.category.map(|c| c.as_str().to_string())
    }
}

const LIST_PREDICATE: &str = "WHERE ($1::text IS NULL OR status = $1) \
     AND ($2::text IS NULL OR category = $2) \
     AND ($3::text IS NULL OR from_address = $3) \
     AND ($4::uuid IS NULL OR lead_id = $4)";

pub async fn list(
    pool: &PgPool,
    filter: &EmailFilter<'_>,
) -> Result<Vec<EmailSummaryRow>, AppError> {
    let sql = format!(
        "SELECT id, thread_id, status, category, spam_verdict, from_address, from_name, \
         subject, received_at, lead_id, size_bytes, attempts, last_error FROM emails \
         {LIST_PREDICATE} ORDER BY received_at DESC, id DESC LIMIT $5 OFFSET $6"
    );
    let rows = sqlx::query_as::<_, EmailSummaryRow>(&sql)
        .bind(filter.status_text())
        .bind(filter.category_text())
        .bind(filter.from_address)
        .bind(filter.lead_id)
        .bind(filter.limit.clamp(1, 500))
        .bind(filter.offset.max(0))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn count(pool: &PgPool, filter: &EmailFilter<'_>) -> Result<i64, AppError> {
    let sql = format!("SELECT count(*) FROM emails {LIST_PREDICATE}");
    let total: i64 = sqlx::query_scalar(&sql)
        .bind(filter.status_text())
        .bind(filter.category_text())
        .bind(filter.from_address)
        .bind(filter.lead_id)
        .fetch_one(pool)
        .await?;
    Ok(total)
}

pub async fn find(pool: &PgPool, id: EmailId) -> Result<Option<StoredEmail>, AppError> {
    let sql = format!("SELECT {EMAIL_COLUMNS} FROM emails WHERE id = $1");
    let row = sqlx::query_as::<_, EmailRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(to_stored).transpose()
}

pub async fn get(pool: &PgPool, id: EmailId) -> Result<StoredEmail, AppError> {
    find(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("email {id} not found")))
}

/// Claim up to `limit` unprocessed messages.
///
/// `FOR UPDATE SKIP LOCKED` lets several workers share the queue without ever
/// handing the same message to two of them.
pub async fn claim_batch(
    pool: &PgPool,
    limit: i64,
    max_attempts: i32,
) -> Result<Vec<EmailRow>, AppError> {
    let sql = format!(
        "UPDATE emails SET status = 'processing' WHERE id IN ( \
           SELECT id FROM emails WHERE status IN ('pending','failed') AND attempts < $2 \
           ORDER BY received_at FOR UPDATE SKIP LOCKED LIMIT $1) \
         RETURNING {EMAIL_COLUMNS}"
    );
    let rows = sqlx::query_as::<_, EmailRow>(&sql)
        .bind(limit.clamp(1, 500))
        .bind(max_attempts.max(0))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn thread_messages(
    pool: &PgPool,
    thread_id: ThreadId,
) -> Result<Vec<StoredEmail>, AppError> {
    let sql =
        format!("SELECT {EMAIL_COLUMNS} FROM emails WHERE thread_id = $1 ORDER BY received_at");
    let rows = sqlx::query_as::<_, EmailRow>(&sql)
        .bind(thread_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(to_stored).collect()
}

pub async fn thread_of(pool: &PgPool, id: EmailId) -> Result<Option<ThreadId>, AppError> {
    let row = sqlx::query_scalar::<_, Uuid>("SELECT thread_id FROM emails WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Whether an address already participates in a thread with this subject.
///
/// Used to detect an existing client conversation before creating a lead, which
/// is what keeps reprocessing from producing a second lead.
pub async fn find_conversation(
    pool: &PgPool,
    address: &str,
    normalized_subject: &str,
) -> Result<Option<EmailId>, AppError> {
    let sql = "SELECT id FROM emails WHERE from_address = $1 AND normalized_subject = $2 \
               AND direction = 'inbound' ORDER BY received_at DESC LIMIT 1";
    let id = sqlx::query_scalar::<_, Uuid>(sql)
        .bind(address)
        .bind(normalized_subject)
        .fetch_optional(pool)
        .await?;
    Ok(id)
}

/// Bodies of recent messages, newest first, for building a prompt context.
pub async fn recent_bodies(
    pool: &PgPool,
    thread_id: ThreadId,
    limit: i64,
) -> Result<Vec<(String, String, bool, chrono::DateTime<chrono::Utc>)>, AppError> {
    let sql = "SELECT subject, text_body, direction = 'outbound' AS is_outbound, received_at \
         FROM emails WHERE thread_id = $1 ORDER BY received_at DESC LIMIT $2"
        .to_string();
    let rows = sqlx::query_as::<_, (String, String, bool, chrono::DateTime<chrono::Utc>)>(&sql)
        .bind(thread_id)
        .bind(limit.clamp(1, 50))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Read a stored message back as an untrusted input envelope.
pub async fn as_inbound(pool: &PgPool, email: &StoredEmail) -> Result<InboundMessage, AppError> {
    Ok(InboundMessage {
        provider_message_id: email.provider_uid.clone().unwrap_or_default(),
        internet_message_id: email.internet_message_id.clone(),
        in_reply_to: email.in_reply_to.clone(),
        references: email.references.clone(),
        from: crate::domain::EmailAddress::with_name(
            email.from_address.clone(),
            email.from_name.clone(),
        ),
        to: email
            .to_addresses
            .iter()
            .map(|a| crate::domain::EmailAddress::new(a.clone()))
            .collect(),
        cc: email
            .cc_addresses
            .iter()
            .map(|a| crate::domain::EmailAddress::new(a.clone()))
            .collect(),
        subject: email.subject.clone(),
        date: email.date,
        text_body: email.text_body.clone(),
        attachments: stored_attachments(pool, email.id).await?,
        total_size: email.size_bytes as usize,
    })
}

/// Rehydrate stored attachment extracts for a reprocess.
///
/// Only the truncated excerpt is returned, never raw bytes, and the excerpt
/// keeps its stored bound so a reprocess cannot be used to smuggle a larger
/// document into a prompt than ingestion allowed.
async fn stored_attachments(
    pool: &PgPool,
    email_id: EmailId,
) -> Result<Vec<EmailAttachment>, AppError> {
    let rows = super::attachments::attachments(pool, email_id).await?;
    Ok(rows
        .into_iter()
        .filter(|row| row.text_excerpt.is_some())
        .map(|row| EmailAttachment {
            filename: row.filename,
            mime_type: row.mime_type,
            size: row.size_bytes.max(0) as usize,
            content_id: row.content_id,
            is_inline: row.is_inline,
            sha256: row.sha256,
            extraction: ExtractionStatus::Extracted {
                characters: row.extracted_chars.max(0) as usize,
            },
            text_excerpt: row.text_excerpt,
        })
        .collect())
}

pub(crate) fn to_stored(row: EmailRow) -> Result<StoredEmail, AppError> {
    Ok(StoredEmail {
        id: row.id,
        thread_id: row.thread_id,
        status: parse_enum(&row.status, "emails.status")?,
        direction: parse_enum(&row.direction, "emails.direction")?,
        provider: row.provider,
        mailbox: row.mailbox,
        provider_uid: row.provider_uid,
        provider_uid_validity: row.provider_uid_validity,
        internet_message_id: row.internet_message_id,
        in_reply_to: row.in_reply_to,
        references: row.references,
        from_address: row.from_address,
        from_name: row.from_name,
        to_addresses: row.to_addresses,
        cc_addresses: row.cc_addresses,
        subject: row.subject,
        normalized_subject: row.normalized_subject,
        date: row.date,
        received_at: row.received_at,
        text_body: row.text_body,
        html_present: row.html_present,
        size_bytes: row.size_bytes,
        category: parse_enum_opt(row.category.as_deref(), "emails.category")?,
        spam_verdict: parse_enum_opt(row.spam_verdict.as_deref(), "emails.spam_verdict")?,
        lead_id: row.lead_id,
        labels: row.labels,
        mailbox_action: parse_enum(&row.mailbox_action, "emails.mailbox_action")?,
        last_error: row.last_error,
        processed_at: row.processed_at,
    })
}
