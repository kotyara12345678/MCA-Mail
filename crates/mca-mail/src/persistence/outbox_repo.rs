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
        /// An outbound email awaiting the send worker's policy check.
        Send => "send",
    }
}

crate::domain::wire_enum! {
    /// What kind of outbound email this row carries.
    OutboundKind {
        CustomerReply => "customer_reply",
        ManagerCard => "manager_card",
    }
}

crate::domain::wire_enum! {
    OutboundStatus {
        Queued => "queued",
        Sending => "sending",
        Sent => "sent",
        Failed => "failed",
        /// Permanently blocked by policy; kept for the audit trail, never retried.
        Held => "held",
        Cancelled => "cancelled",
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

/// One queued outbound email. The row *is* the intent: nothing sends before it
/// exists, and nothing sends twice no matter how often the worker restarts.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboundRow {
    pub id: Uuid,
    pub email_id: Option<Uuid>,
    pub lead_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub message_type: String,
    pub recipient: String,
    pub in_reply_to: Option<String>,
    pub ref_headers: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub body_html: String,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_attempt_at: chrono::DateTime<chrono::Utc>,
    pub last_error: Option<String>,
    pub idempotency_key: Option<String>,
    pub correlation_id: Option<String>,
}

impl OutboundRow {
    pub fn kind(&self) -> Result<OutboundKind, AppError> {
        super::parse_enum(&self.message_type, "mailbox_outbox.message_type").map_err(Into::into)
    }

    pub fn status(&self) -> Result<OutboundStatus, AppError> {
        super::parse_enum(&self.status, "mailbox_outbox.status").map_err(Into::into)
    }
}

const SEND_COLUMNS: &str = "id, email_id, lead_id, run_id, message_type, recipient, in_reply_to, \
     ref_headers, subject, body_text, body_html, status, attempts, max_attempts, \
     next_attempt_at, last_error, idempotency_key, correlation_id";

/// Everything needed to decide whether one queued message may go out.
pub struct OutboundIntent {
    pub message_type: OutboundKind,
    pub lead_id: Option<Uuid>,
    pub email_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub recipient: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: String,
    pub in_reply_to: Option<String>,
    pub ref_headers: Vec<String>,
    pub idempotency_key: String,
    pub correlation_id: Option<String>,
}

impl Default for OutboundIntent {
    fn default() -> Self {
        Self {
            message_type: OutboundKind::CustomerReply,
            lead_id: None,
            email_id: None,
            run_id: None,
            recipient: String::new(),
            subject: String::new(),
            body_text: String::new(),
            body_html: String::new(),
            in_reply_to: None,
            ref_headers: Vec::new(),
            idempotency_key: String::new(),
            correlation_id: None,
        }
    }
}

/// Queue an outbound email.
///
/// `ON CONFLICT DO NOTHING` is the whole guarantee: a pipeline that runs
/// twice, a duplicate event or a worker restarted mid-write all land on the
/// same row and send once. No conflict target is named on purpose — a row can
/// lose on the idempotency key *or* on `uq_outbox_manager_card_per_lead`,
/// which allows one card per lead and recipient and therefore also covers the
/// rows queued before the key learned the address. Losing either way means
/// the winner already carries this message.
pub async fn enqueue_send(
    pool: &PgPool,
    intent: &OutboundIntent,
) -> Result<Option<Uuid>, AppError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO mailbox_outbox (email_id, action, message_type, lead_id, run_id, recipient, \
         in_reply_to, ref_headers, subject, body_text, body_html, idempotency_key, \
         correlation_id, status) \
         VALUES ($1,'send',$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'queued') \
         ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(intent.email_id)
    .bind(intent.message_type.as_str())
    .bind(intent.lead_id)
    .bind(intent.run_id)
    .bind(intent.recipient.trim().to_ascii_lowercase())
    .bind(&intent.in_reply_to)
    .bind(&intent.ref_headers)
    .bind(&intent.subject)
    .bind(&intent.body_text)
    .bind(&intent.body_html)
    .bind(&intent.idempotency_key)
    .bind(&intent.correlation_id)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

/// Claim the sends that are due, one batch at a time.
///
/// `FOR UPDATE SKIP LOCKED` means several workers (or a restarted one racing
/// its own predecessor) never pick up the same row.
pub async fn claim_due(pool: &PgPool, limit: i64) -> Result<Vec<OutboundRow>, AppError> {
    let rows = sqlx::query_as::<_, OutboundRow>(&format!(
        "UPDATE mailbox_outbox SET status = 'sending', claimed_at = now() \
         WHERE id IN (SELECT id FROM mailbox_outbox \
                      WHERE action = 'send' AND applied = FALSE \
                        AND status IN ('queued','failed') \
                        AND next_attempt_at <= now() \
                      ORDER BY next_attempt_at \
                      FOR UPDATE SKIP LOCKED \
                      LIMIT $1) \
         RETURNING {SEND_COLUMNS}"
    ))
    .bind(limit.clamp(1, 50))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_sent(
    pool: &PgPool,
    id: Uuid,
    provider_message_id: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE mailbox_outbox SET status = 'sent', applied = TRUE, attempts = attempts + 1, \
         sent_at = now(), provider_message_id = $2, last_error = NULL \
         WHERE id = $1",
    )
    .bind(id)
    .bind(provider_message_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Transient failure: back off exponentially with jitter, and stop for good
/// once the row has used its budget.
pub async fn mark_retry(
    pool: &PgPool,
    id: Uuid,
    attempts: i32,
    max_attempts: i32,
    error: &str,
    delay_seconds: i64,
) -> Result<(), AppError> {
    let terminal = attempts >= max_attempts;
    sqlx::query(
        "UPDATE mailbox_outbox SET \
           attempts = $2, \
           last_error = $3, \
           status = CASE WHEN $4 THEN 'failed' ELSE 'queued' END, \
           next_attempt_at = now() + make_interval(secs => $5), \
           WHERE id = $1",
    )
    .bind(id)
    .bind(attempts)
    .bind(crate::observability::errors::clip(error, 300))
    .bind(terminal)
    .bind(delay_seconds as f64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Policy said no, and no amount of waiting will change that.
pub async fn mark_held(pool: &PgPool, id: Uuid, denial: &str, error: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE mailbox_outbox SET status = 'held', policy_denial = $2, last_error = $3 \
         WHERE id = $1",
    )
    .bind(id)
    .bind(denial)
    .bind(crate::observability::errors::clip(error, 300))
    .execute(pool)
    .await?;
    Ok(())
}

/// A pacing limit will expire on its own: put the row back with a deadline.
pub async fn reschedule(
    pool: &PgPool,
    id: Uuid,
    denial: &str,
    delay_seconds: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE mailbox_outbox SET status = 'queued', policy_denial = $2, \
         next_attempt_at = now() + make_interval(secs => $3) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(denial)
    .bind(delay_seconds as f64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Release a row back to `queued` after a crash between claim and decision.
pub async fn release(pool: &PgPool, id: Uuid) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE mailbox_outbox SET status = 'queued' \
         WHERE id = $1 AND status = 'sending'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Hand back every send whose worker never came back.
///
/// Called once per tick rather than inside `claim_due`, so a batch that is
/// simply taking its time over a slow SMTP round-trip is not stolen from the
/// worker that owns it.
pub async fn release_stale(pool: &PgPool, older_than_seconds: i64) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE mailbox_outbox SET status = 'queued', claimed_at = NULL \
         WHERE action = 'send' AND status = 'sending' \
           AND (claimed_at IS NULL OR claimed_at < now() - make_interval(secs => $1))",
    )
    .bind(older_than_seconds as f64)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Already-sent rows for a lead, so a card cannot be created twice.
pub async fn has_sent(pool: &PgPool, idempotency_key: &str) -> Result<bool, AppError> {
    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM mailbox_outbox WHERE idempotency_key = $1 AND status <> 'cancelled' LIMIT 1",
    )
    .bind(idempotency_key)
    .fetch_optional(pool)
    .await?;
    Ok(found.is_some())
}

/// How much has already gone out, for the outbound policy's pacing checks.
///
/// Counted from this table rather than from `emails`, because the outbox is
/// the single place a send actually happens: counting a second source would
/// let a message queued before a restart slip under the hourly ceiling.
#[derive(Debug, Clone, Copy, Default)]
pub struct SendStats {
    /// Messages delivered to this address in the last hour.
    pub recipient_last_hour: i64,
    /// Messages delivered to anyone in the last hour.
    pub total_last_hour: i64,
    /// When this address was last contacted, however long ago.
    pub last_sent_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn send_stats(pool: &PgPool, recipient: &str) -> Result<SendStats, AppError> {
    let row = sqlx::query_as::<_, (i64, i64, Option<chrono::DateTime<chrono::Utc>>)>(
        "SELECT count(*) FILTER (WHERE recipient = $1 AND sent_at > now() - interval '1 hour'), \
                count(*) FILTER (WHERE sent_at > now() - interval '1 hour'), \
                max(sent_at) FILTER (WHERE recipient = $1) \
         FROM mailbox_outbox \
         WHERE action = 'send' AND status = 'sent' AND sent_at IS NOT NULL",
    )
    .bind(recipient.trim().to_ascii_lowercase())
    .fetch_one(pool)
    .await?;
    Ok(SendStats {
        recipient_last_hour: row.0,
        total_last_hour: row.1,
        last_sent_at: row.2,
    })
}

/// Manager cards already delivered to a recipient in the last hour, so the
/// card's own hourly ceiling does not have to borrow the customer-reply one.
pub async fn manager_cards_last_hour(pool: &PgPool, recipient: &str) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailbox_outbox \
         WHERE action = 'send' AND status = 'sent' AND message_type = 'manager_card' \
           AND recipient = $1 AND sent_at > now() - interval '1 hour'",
    )
    .bind(recipient.trim().to_ascii_lowercase())
    .fetch_one(pool)
    .await?;
    Ok(count)
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
         FROM mailbox_outbox WHERE applied = FALSE AND action <> 'send' \
         ORDER BY created_at LIMIT $1",
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
