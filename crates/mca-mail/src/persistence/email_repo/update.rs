use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{EmailCategory, EmailId, EmailStatus, MailboxAction, SpamVerdict};
use crate::error::AppError;

pub async fn set_status(
    pool: &PgPool,
    id: EmailId,
    status: EmailStatus,
    error: Option<&str>,
) -> Result<(), AppError> {
    let done = sqlx::query(
        "UPDATE emails SET status = $2, last_error = $3, \
             processed_at = CASE WHEN $2 IN ('processed','quarantined','dead') \
                                 THEN now() ELSE processed_at END \
         WHERE id = $1",
    )
    .bind(id)
    .bind(status.as_str())
    .bind(error.map(|e| truncate(e, 2000)))
    .execute(pool)
    .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("email {id} not found")));
    }
    Ok(())
}

pub async fn set_classification(
    pool: &PgPool,
    id: EmailId,
    category: EmailCategory,
    spam: SpamVerdict,
) -> Result<(), AppError> {
    sqlx::query("UPDATE emails SET category = $2, spam_verdict = $3 WHERE id = $1")
        .bind(id)
        .bind(category.as_str())
        .bind(spam.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

/// Store just the spam verdict, leaving the category untouched.
pub async fn set_spam_verdict(
    pool: &PgPool,
    id: EmailId,
    spam: SpamVerdict,
) -> Result<(), AppError> {
    sqlx::query("UPDATE emails SET spam_verdict = $2 WHERE id = $1")
        .bind(id)
        .bind(spam.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

/// Store just the category, leaving the spam verdict untouched.
pub async fn set_category(
    pool: &PgPool,
    id: EmailId,
    category: EmailCategory,
) -> Result<(), AppError> {
    sqlx::query("UPDATE emails SET category = $2 WHERE id = $1")
        .bind(id)
        .bind(category.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_mailbox_action(
    pool: &PgPool,
    id: EmailId,
    action: MailboxAction,
) -> Result<(), AppError> {
    sqlx::query("UPDATE emails SET mailbox_action = $2 WHERE id = $1")
        .bind(id)
        .bind(action.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn attach_lead(pool: &PgPool, id: EmailId, lead_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE emails SET lead_id = $2 WHERE id = $1")
        .bind(id)
        .bind(lead_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn record_error(pool: &PgPool, id: EmailId, error: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE emails SET attempts = attempts + 1, last_error = $2 WHERE id = $1")
        .bind(id)
        .bind(truncate(error, 2000))
        .execute(pool)
        .await?;
    Ok(())
}

/// Release claimed messages back to the queue after a transient failure.
pub async fn release(pool: &PgPool, id: EmailId, error: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE emails SET status = 'failed', last_error = $2 WHERE id = $1")
        .bind(id)
        .bind(truncate(error, 2000))
        .execute(pool)
        .await?;
    Ok(())
}

/// Replace the body with a placeholder. Used by the retention job so lead
/// history stays coherent while personal data ages out.
pub async fn anonymize(pool: &PgPool, older_than_days: i32) -> Result<u64, AppError> {
    let done = sqlx::query(
        "UPDATE emails SET text_body = '[anonymized by retention policy]', anonymized_at = now() \
         WHERE text_body <> '' AND anonymized_at IS NULL AND direction = 'inbound' \
         AND received_at < now() - make_interval(days => $1)",
    )
    .bind(older_than_days)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

pub fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}
