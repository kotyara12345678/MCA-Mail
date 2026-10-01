use sqlx::PgPool;

use super::rows::ThreadRow;
use crate::domain::{EmailAddress, EmailThread, ThreadId};
use crate::error::AppError;

/// Stable conversation key.
///
/// Two messages belong to the same thread when the reply headers chain them, or
/// when the same address writes about the same normalized subject. This is the
/// key that makes a lead idempotent across reprocessing.
pub fn conversation_key(addresses: &[&EmailAddress], normalized_subject: &str) -> String {
    let mut participants: Vec<String> = addresses
        .iter()
        .filter(|a| !a.address.is_empty())
        .map(|a| a.address.clone())
        .collect();
    participants.sort();
    participants.dedup();
    // Join before the separator suffix so a single-participant key is just the
    // address: `a@example.com::Subject`, not `a@example.com|::Subject`.
    let mut key = participants.join("|");
    if !normalized_subject.is_empty() {
        key.push_str("::");
        key.push_str(normalized_subject);
    }
    key
}

/// Get the thread for a conversation key, creating it when absent.
pub async fn ensure_thread(
    pool: &PgPool,
    key: &str,
    normalized_subject: &str,
    root_message_id: Option<&str>,
) -> Result<ThreadId, AppError> {
    let existing = sqlx::query_scalar::<_, ThreadId>(
        "SELECT id FROM email_threads WHERE conversation_key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = sqlx::query_scalar::<_, ThreadId>(
        "INSERT INTO email_threads (root_message_id, normalized_subject, conversation_key) \
         VALUES ($1, $2, $3) ON CONFLICT (conversation_key) DO UPDATE \
         SET updated_at = now() RETURNING id",
    )
    .bind(root_message_id)
    .bind(normalized_subject)
    .bind(key)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Find the thread a reply belongs to by its RFC 5322 `Message-ID` or by the
/// provider message identifier the gateway assigned to a stored message.
pub async fn find_by_reference(
    pool: &PgPool,
    references: &[String],
) -> Result<Option<ThreadId>, AppError> {
    for reference in references.iter().take(8) {
        let normalized = reference.trim();
        if normalized.is_empty() {
            continue;
        }
        let found = sqlx::query_scalar::<_, ThreadId>(
            "SELECT t.id FROM email_threads t JOIN emails e ON e.thread_id = t.id \
             WHERE e.internet_message_id = $1 OR e.provider_uid = $1 LIMIT 1",
        )
        .bind(normalized)
        .fetch_optional(pool)
        .await?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

pub async fn get(pool: &PgPool, id: ThreadId) -> Result<Option<EmailThread>, AppError> {
    let row = sqlx::query_as::<_, ThreadRow>(
        "SELECT id, root_message_id, normalized_subject, conversation_key, created_at, updated_at \
         FROM email_threads WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| EmailThread {
        id: r.id,
        root_message_id: r.root_message_id,
        normalized_subject: r.normalized_subject,
        conversation_key: r.conversation_key,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

pub async fn touch(pool: &PgPool, id: ThreadId) -> Result<(), AppError> {
    sqlx::query("UPDATE email_threads SET updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_key_is_order_independent() {
        let a = EmailAddress::new("b@example.com");
        let b = EmailAddress::new("a@example.com");
        let first = conversation_key(&[&a, &b], "Заявка");
        let second = conversation_key(&[&b, &a], "Заявка");
        assert_eq!(first, second);
        assert!(first.ends_with("::Заявка"));
    }

    #[test]
    fn conversation_key_deduplicates_addresses() {
        let a = EmailAddress::new("a@example.com");
        let single = conversation_key(&[&a], "Тема");
        let repeated = conversation_key(&[&a, &a], "Тема");
        // Repeating the same address must not change the key, or a duplicate
        // To/Cc entry would create a second lead for one conversation.
        assert_eq!(single, repeated);
        assert_eq!(single, "a@example.com::Тема");
    }
}
