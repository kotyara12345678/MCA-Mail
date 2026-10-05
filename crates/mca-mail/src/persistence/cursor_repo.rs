//! Fresh-mail cursor: UIDVALIDITY + high-water mark, one row per mailbox.
//!
//! The poll worker consults this before every fetch: the row decides whether a
//! cycle may read at all, and it only ever moves forward. A UIDVALIDITY change
//! rewrites the boundary instead of replaying history — the whole point of the
//! row is that "new" survives restarts without a backfill.

use sqlx::PgPool;

use crate::error::AppError;

/// Stored boundary for one mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxCursor {
    pub uid_validity: i64,
    pub high_water_uid: i64,
}

/// Read the boundary, if the mailbox has ever been bounded.
pub async fn load(pool: &PgPool, mailbox: &str) -> Result<Option<MailboxCursor>, AppError> {
    let row = sqlx::query_as::<_, (i64, i64)>(
        "SELECT uid_validity, high_water_uid FROM mailbox_cursors WHERE mailbox = $1",
    )
    .bind(mailbox)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(uid_validity, high_water_uid)| MailboxCursor {
        uid_validity,
        high_water_uid,
    }))
}

/// Record the boundary on the first cycle.
///
/// `DO NOTHING` because two pollers racing to initialize must not move the
/// edge twice; whichever write lands first defines the mailbox.
pub async fn initialize(
    pool: &PgPool,
    mailbox: &str,
    uid_validity: u32,
    edge: u32,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO mailbox_cursors (mailbox, uid_validity, high_water_uid) \
         VALUES ($1, $2, $3) ON CONFLICT (mailbox) DO NOTHING",
    )
    .bind(mailbox)
    .bind(i64::from(uid_validity))
    .bind(i64::from(edge))
    .execute(pool)
    .await?;
    Ok(())
}

/// Re-bound after a UIDVALIDITY change: nothing currently on the server is
/// treated as new, and no message seen under the old validity is fetched again.
///
/// The `WHERE` keeps a concurrent reset (same validity) from overwriting a
/// mark that has already advanced past the edge it would write.
pub async fn reset(
    pool: &PgPool,
    mailbox: &str,
    uid_validity: u32,
    edge: u32,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO mailbox_cursors (mailbox, uid_validity, high_water_uid) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (mailbox) DO UPDATE \
         SET uid_validity = EXCLUDED.uid_validity, \
             high_water_uid = EXCLUDED.high_water_uid, \
             updated_at = now() \
         WHERE mailbox_cursors.uid_validity IS DISTINCT FROM EXCLUDED.uid_validity",
    )
    .bind(mailbox)
    .bind(i64::from(uid_validity))
    .bind(i64::from(edge))
    .execute(pool)
    .await?;
    Ok(())
}

/// Move the boundary forward after a fully stored batch.
///
/// Refuses to move a cursor written under a different UIDVALIDITY and never
/// moves backwards. Returns whether anything actually advanced, so a caller can
/// tell "already ahead" from "someone re-bound the mailbox".
pub async fn advance(
    pool: &PgPool,
    mailbox: &str,
    uid_validity: u32,
    highest: u32,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "INSERT INTO mailbox_cursors (mailbox, uid_validity, high_water_uid) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (mailbox) DO UPDATE \
         SET high_water_uid = EXCLUDED.high_water_uid, updated_at = now() \
         WHERE mailbox_cursors.uid_validity = EXCLUDED.uid_validity \
           AND mailbox_cursors.high_water_uid < EXCLUDED.high_water_uid",
    )
    .bind(mailbox)
    .bind(i64::from(uid_validity))
    .bind(i64::from(highest))
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
