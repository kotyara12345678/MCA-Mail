use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{EmailId, InboundMessage};
use crate::error::AppError;

/// Stable across mailbox-local UID changes, with content guarding duplicate IDs.
pub fn dedup_key_for(message: &InboundMessage) -> String {
    let digest = content_digest(message);
    match message.internet_message_id.as_deref() {
        Some(mid) => format!("mid:{}:sha:{digest}", mid.trim().to_ascii_lowercase()),
        None => format!("sha:{digest}"),
    }
}

pub fn scoped_dedup_key_for(mailbox: &str, message: &InboundMessage) -> String {
    format!(
        "mailbox:{}:{}",
        mailbox.trim().to_ascii_lowercase(),
        dedup_key_for(message)
    )
}

fn content_digest(message: &InboundMessage) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(message.from.address.as_bytes());
    hasher.update(b"|");
    hasher.update(message.subject.as_bytes());
    hasher.update(b"|");
    hasher.update(message.date.map(|d| d.to_rfc3339()).unwrap_or_default());
    hasher.update(b"|");
    hasher.update(message.text_body.as_bytes());
    for attachment in &message.attachments {
        hasher.update(attachment.sha256.as_bytes());
    }
    hex::encode(hasher.finalize())
}

pub(super) async fn legacy_duplicate(
    pool: &PgPool,
    mailbox: &str,
    message: &InboundMessage,
) -> Result<Option<EmailId>, AppError> {
    let hashes: Vec<String> = message
        .attachments
        .iter()
        .map(|a| a.sha256.clone())
        .collect();
    let id = sqlx::query_scalar::<_, Uuid>(
        "SELECT e.id FROM emails e WHERE e.internet_message_id IS NOT DISTINCT FROM $1 \
         AND e.from_address = $2 AND e.subject = $3 AND e.date IS NOT DISTINCT FROM $4 \
         AND e.text_body = $5 AND e.size_bytes = $6 \
         AND ARRAY(SELECT a.sha256 FROM email_attachments a WHERE a.email_id = e.id ORDER BY a.sha256) \
             = ARRAY(SELECT h FROM unnest($7::text[]) h ORDER BY h) \
         AND e.mailbox = $8 \
         ORDER BY e.received_at LIMIT 1",
    )
    .bind(message.internet_message_id.as_deref())
    .bind(&message.from.address)
    .bind(&message.subject)
    .bind(message.date)
    .bind(&message.text_body)
    .bind(message.total_size as i64)
    .bind(hashes)
    .bind(mailbox)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

#[cfg(test)]
#[path = "dedup_test.rs"]
mod tests;
