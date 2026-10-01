use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::EmailId;
use crate::error::AppError;

crate::domain::wire_enum! {
    OutboxAction {
        Move => "move",
        Label => "label",
        MarkRead => "mark_read",
        Archive => "archive",
        Flag => "flag",
    }
}

/// A mailbox mutation that policy has not (yet) allowed to execute.
///
/// In `dry_run` and `review` modes every folder move and label ends up here
/// instead of touching the corporate mailbox, which is what makes those modes
/// safe to point at production mail.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxItem {
    pub id: Uuid,
    pub email_id: EmailId,
    pub action: String,
    pub target: Option<String>,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn enqueue(
    pool: &PgPool,
    email_id: EmailId,
    action: OutboxAction,
    target: Option<&str>,
) -> Result<Uuid, AppError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO mailbox_outbox (email_id, action, target) VALUES ($1,$2,$3) \
         ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(email_id)
    .bind(action.as_str())
    .bind(target)
    .fetch_optional(pool)
    .await?;
    Ok(id.unwrap_or_else(Uuid::new_v4))
}

pub async fn pending(pool: &PgPool, limit: i64) -> Result<Vec<OutboxItem>, AppError> {
    let rows = sqlx::query_as::<_, OutboxItem>(
        "SELECT id, email_id, action, target, attempts, last_error, created_at \
         FROM mailbox_outbox WHERE applied = FALSE ORDER BY created_at LIMIT $1",
    )
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_applied(pool: &PgPool, id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE mailbox_outbox SET applied = TRUE, applied_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_failed(pool: &PgPool, id: Uuid, error: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE mailbox_outbox SET attempts = attempts + 1, last_error = $2 WHERE id = $1")
        .bind(id)
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn pending_count(pool: &PgPool) -> Result<i64, AppError> {
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mailbox_outbox WHERE applied = FALSE")
            .fetch_one(pool)
            .await?;
    Ok(total)
}
