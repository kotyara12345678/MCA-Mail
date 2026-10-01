use sqlx::PgPool;
use uuid::Uuid;

use super::parse_enum;
use crate::domain::{
    ConversationDirection, ConversationEntry, ConversationId, EmailId, LeadId, OutboundState,
};
use crate::error::AppError;

#[derive(Debug, sqlx::FromRow)]
struct ConversationRow {
    id: Uuid,
    lead_id: Uuid,
    email_id: Option<Uuid>,
    direction: String,
    state: String,
    subject: String,
    body: String,
    idempotency_key: String,
    sent_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
}

fn to_domain(row: ConversationRow) -> Result<ConversationEntry, AppError> {
    Ok(ConversationEntry {
        id: row.id,
        lead_id: row.lead_id,
        email_id: row.email_id,
        direction: parse_enum::<ConversationDirection>(
            &row.direction,
            "lead_conversations.direction",
        )?,
        state: parse_enum::<OutboundState>(&row.state, "lead_conversations.state")?,
        subject: row.subject,
        body: row.body,
        idempotency_key: row.idempotency_key,
        sent_at: row.sent_at,
        created_at: row.created_at,
    })
}

const COLUMNS: &str = "id, lead_id, email_id, direction, state, subject, body, \
     idempotency_key, sent_at, created_at";

pub async fn history(
    pool: &PgPool,
    lead_id: LeadId,
    limit: i64,
) -> Result<Vec<ConversationEntry>, AppError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM lead_conversations WHERE lead_id = $1 \
         ORDER BY created_at DESC, id DESC LIMIT $2"
    );
    let rows = sqlx::query_as::<_, ConversationRow>(&sql)
        .bind(lead_id)
        .bind(limit.clamp(1, 200))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(to_domain).collect()
}

pub async fn append(pool: &PgPool, entry: &ConversationEntry) -> Result<ConversationId, AppError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO lead_conversations (lead_id, email_id, direction, state, subject, body, \
         idempotency_key, sent_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
         ON CONFLICT (idempotency_key) DO UPDATE SET created_at = lead_conversations.created_at \
         RETURNING id",
    )
    .bind(entry.lead_id)
    .bind(entry.email_id)
    .bind(entry.direction.as_str())
    .bind(entry.state.as_str())
    .bind(&entry.subject)
    .bind(&entry.body)
    .bind(&entry.idempotency_key)
    .bind(entry.sent_at)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Mark a conversation entry as actually sent, which is what stops a reprocess
/// from counting the same message twice.
pub async fn mark_sent(
    pool: &PgPool,
    lead_id: LeadId,
    idempotency_key: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE lead_conversations SET state = 'sent', sent_at = now() \
         WHERE lead_id = $1 AND idempotency_key = $2",
    )
    .bind(lead_id)
    .bind(idempotency_key)
    .execute(pool)
    .await?;
    Ok(())
}

/// Questions already asked, so the agent never repeats itself.
pub async fn asked_questions(pool: &PgPool, lead_id: LeadId) -> Result<Vec<String>, AppError> {
    let bodies = sqlx::query_scalar::<_, String>(
        "SELECT body FROM lead_conversations WHERE lead_id = $1 AND direction = 'outbound' \
         ORDER BY created_at DESC LIMIT 20",
    )
    .bind(lead_id)
    .fetch_all(pool)
    .await?;
    Ok(bodies)
}

pub async fn outbound_count(pool: &PgPool, lead_id: LeadId) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM lead_conversations WHERE lead_id = $1 \
         AND direction = 'outbound' AND state = 'sent'",
    )
    .bind(lead_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Longest run of consecutive outbound messages with no customer reply.
pub async fn unanswered_replies(pool: &PgPool, lead_id: LeadId) -> Result<i64, AppError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT direction FROM lead_conversations WHERE lead_id = $1 \
         ORDER BY created_at DESC, id DESC LIMIT 100",
    )
    .bind(lead_id)
    .fetch_all(pool)
    .await?;
    let mut streak = 0i64;
    for direction in rows {
        if direction == "outbound" {
            streak += 1;
        } else {
            break;
        }
    }
    Ok(streak)
}

pub async fn add_note(
    pool: &PgPool,
    lead_id: LeadId,
    body: &str,
    key: &str,
) -> Result<Uuid, AppError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO lead_conversations (lead_id, direction, state, subject, body, \
         idempotency_key) VALUES ($1,'inbound','approved','служебная заметка',$2,$3) \
         ON CONFLICT (idempotency_key) DO UPDATE SET body = lead_conversations.body \
         RETURNING id",
    )
    .bind(lead_id)
    .bind(body)
    .bind(key)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn attach_email(
    pool: &PgPool,
    lead_id: LeadId,
    email_id: EmailId,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE lead_conversations SET email_id = $2 WHERE lead_id = $1 AND email_id IS NULL",
    )
    .bind(lead_id)
    .bind(email_id)
    .execute(pool)
    .await?;
    Ok(())
}
