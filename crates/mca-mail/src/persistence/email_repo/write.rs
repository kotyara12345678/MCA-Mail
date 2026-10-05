use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{EmailId, InboundMessage, ThreadId};
use crate::error::AppError;

use super::super::is_unique_violation;
use super::dedup::{legacy_duplicate, scoped_dedup_key_for};

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

fn address_list(list: &[crate::domain::EmailAddress]) -> Vec<String> {
    list.iter().map(|a| a.address.clone()).collect()
}

/// Store an inbound message, or report the existing row when it is a duplicate.
///
/// `uid_validity` is the mailbox's UIDVALIDITY at fetch time. It completes the
/// `(mailbox, provider_uid, provider_uid_validity)` unique key, so a UID that a
/// server re-numbered after a validity change cannot collide with its old row,
/// while a replay of the same UID under the same validity resolves to it.
pub async fn insert_inbound(
    pool: &PgPool,
    thread_id: ThreadId,
    mailbox: &str,
    message: &InboundMessage,
    uid_validity: Option<i64>,
) -> Result<InsertOutcome, AppError> {
    let dedup_key = scoped_dedup_key_for(mailbox, message);
    if let Some(id) = legacy_duplicate(pool, mailbox, message).await? {
        return Ok(InsertOutcome::Duplicate(id));
    }
    let normalized = crate::domain::normalize_subject(&message.subject);
    let sql = "INSERT INTO emails (thread_id, direction, status, provider, mailbox, provider_uid, \
         internet_message_id, in_reply_to, \"references\", from_address, from_name, to_addresses, \
         cc_addresses, subject, normalized_subject, date, text_body, size_bytes, dedup_key, \
         provider_uid_validity) \
         VALUES ($1, 'inbound', 'pending', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
                 $14, $15, $16, $17, $18) RETURNING id"
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
        .bind(uid_validity)
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
