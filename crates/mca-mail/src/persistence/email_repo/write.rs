use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{EmailId, InboundMessage, ThreadId};
use crate::error::AppError;

use super::super::is_unique_violation;

/// Result of trying to record a message that may already be stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted(EmailId),
    /// Already present; nothing was written. Expected when the same message is
    /// fetched again after a restart or a mailbox rescan.
    Duplicate(EmailId),
}

impl InsertOutcome {
    pub fn email_id(&self) -> EmailId {
        match self {
            InsertOutcome::Inserted(id) | InsertOutcome::Duplicate(id) => *id,
        }
    }

    pub fn is_new(&self) -> bool {
        matches!(self, InsertOutcome::Inserted(_))
    }
}

/// Provider message identifier first, RFC 5322 `Message-ID` second, a content
/// digest last. A unique index on this key is what makes ingestion idempotent.
pub fn dedup_key_for(message: &InboundMessage) -> String {
    let provider = message.provider_message_id.trim();
    if !provider.is_empty() {
        return format!("pmid:{provider}");
    }
    if let Some(mid) = message.internet_message_id.as_deref() {
        return format!("mid:{}", mid.trim().to_ascii_lowercase());
    }
    format!("sha:{}", content_digest(message))
}

fn content_digest(message: &InboundMessage) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(message.from.address.as_bytes());
    hasher.update(b"|");
    hasher.update(message.subject.as_bytes());
    hasher.update(b"|");
    hasher.update(
        message
            .date
            .map(|d| d.to_rfc3339())
            .unwrap_or_default()
            .as_bytes(),
    );
    hasher.update(b"|");
    hasher.update(message.text_body.as_bytes());
    hex::encode(hasher.finalize())
}

fn address_list(list: &[crate::domain::EmailAddress]) -> Vec<String> {
    list.iter().map(|a| a.address.clone()).collect()
}

/// Store an inbound message, or report the existing row when it is a duplicate.
pub async fn insert_inbound(
    pool: &PgPool,
    thread_id: ThreadId,
    mailbox: &str,
    message: &InboundMessage,
) -> Result<InsertOutcome, AppError> {
    let dedup_key = dedup_key_for(message);
    let normalized = crate::domain::normalize_subject(&message.subject);
    let sql = "INSERT INTO emails (thread_id, direction, status, provider, mailbox, provider_uid, \
         internet_message_id, in_reply_to, \"references\", from_address, from_name, to_addresses, \
         cc_addresses, subject, normalized_subject, date, text_body, size_bytes, dedup_key) \
         VALUES ($1, 'inbound', 'pending', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
                 $14, $15, $16, $17) RETURNING id"
        .to_string();

    let inserted = sqlx::query_scalar::<_, Uuid>(&sql)
        .bind(thread_id)
        .bind("gateway")
        .bind(mailbox)
        .bind(message.provider_message_id.trim())
        .bind(message.internet_message_id.as_deref())
        .bind(message.in_reply_to.as_deref())
        .bind(&message.references)
        .bind(&message.from.address)
        .bind(message.from.name.as_deref())
        .bind(address_list(&message.to))
        .bind(address_list(&message.cc))
        .bind(&message.subject)
        .bind(&normalized)
        .bind(message.date)
        .bind(&message.text_body)
        .bind(message.total_size as i64)
        .bind(&dedup_key)
        .fetch_optional(pool)
        .await;

    match inserted {
        Ok(Some(id)) => Ok(InsertOutcome::Inserted(id)),
        Ok(None) => duplicate(pool, &dedup_key).await,
        Err(e) if is_unique_violation(&e) => duplicate(pool, &dedup_key).await,
        Err(e) => Err(e.into()),
    }
}

async fn duplicate(pool: &PgPool, dedup_key: &str) -> Result<InsertOutcome, AppError> {
    let id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM emails WHERE dedup_key = $1")
        .bind(dedup_key)
        .fetch_optional(pool)
        .await?;
    match id {
        Some(id) => Ok(InsertOutcome::Duplicate(id)),
        None => Err(AppError::internal(format!(
            "unique violation on dedup key with no matching row (key={dedup_key})"
        ))),
    }
}
