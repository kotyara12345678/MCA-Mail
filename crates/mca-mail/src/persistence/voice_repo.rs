//! Voice-channel persistence: the four operations a call can perform.
//!
//! No new tables. A voice caller maps to a normal `leads` row with source
//! `api` and key `voice:<digits>`; requirements and conversation reuse the
//! existing stores, so a manager reading the CRM sees a voice lead exactly
//! like an email one.

use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    ConversationDirection, ConversationEntry, FieldState, LeadId, LeadRequirement, OutboundState,
    RequirementScope,
};
use crate::error::AppError;
use crate::persistence::{conversation_repo, lead_repo, requirement_repo};
use crate::voice::{phone, ValidatedRequirement};

/// Find or create the lead for a voice caller. Returns `(lead_id, created)`.
///
/// The idempotency guarantee is the unique `conversation_key` (`voice:<digits>`):
/// the same number always lands on the same lead, so replays and follow-up
/// calls keep the thread.
pub async fn ensure_voice_lead(
    pool: &PgPool,
    raw_number: &str,
    scope: RequirementScope,
) -> Result<(LeadId, bool), AppError> {
    let digits = phone::normalize_phone(raw_number)?;
    let key = phone::voice_key(raw_number);
    if let Some(id) = lead_repo::find_by_key(pool, &key).await? {
        return Ok((id, false));
    }
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO leads (source, status, scope, conversation_key, contact_email, callback_phone) \
         VALUES ('api','new',$1,$2,$3,$4) RETURNING id",
    )
    .bind(scope.as_str())
    .bind(&key)
    .bind(&digits)
    .bind(&digits)
    .fetch_one(pool)
    .await?;
    seed_scope(pool, id, scope).await?;
    Ok((id, true))
}

/// Align `lead_requirements` rows with the requested service scope: fields the
/// scope never asks about become `not_applicable`, the rest are seeded as
/// `unknown` so the manager always sees the full picture.
pub async fn seed_scope(
    pool: &PgPool,
    lead_id: LeadId,
    scope: RequirementScope,
) -> Result<(), AppError> {
    let mut applicable = Vec::new();
    let mut not_applicable = Vec::new();
    for field in crate::domain::RequirementField::ALL {
        if field.applies_to(scope) {
            applicable.push(*field);
        } else {
            not_applicable.push(*field);
        }
    }
    requirement_repo::mark_not_applicable(pool, lead_id, &not_applicable).await?;
    requirement_repo::seed_missing(pool, lead_id, &applicable).await?;
    Ok(())
}

/// Change the service scope on the lead and realign the requirement rows.
pub async fn update_scope(
    pool: &PgPool,
    lead_id: LeadId,
    scope: RequirementScope,
) -> Result<(), AppError> {
    sqlx::query("UPDATE leads SET scope = $2 WHERE id = $1")
        .bind(lead_id)
        .bind(scope.as_str())
        .execute(pool)
        .await?;
    seed_scope(pool, lead_id, scope).await
}

/// Persist validated requirements as customer-stated facts.
pub async fn save_requirements(
    pool: &PgPool,
    lead_id: LeadId,
    items: &[ValidatedRequirement],
) -> Result<u64, AppError> {
    let requirements: Vec<LeadRequirement> = items
        .iter()
        .map(|item| LeadRequirement {
            lead_id,
            field: item.field,
            value: Some(item.value.clone()),
            state: FieldState::Known,
            source: crate::domain::RequirementSource::Customer,
            unit: item.unit.clone(),
            confidence: Some(1.0),
            evidence: None,
            updated_at: chrono::Utc::now(),
        })
        .collect();
    requirement_repo::upsert_many(pool, lead_id, &requirements).await
}

/// Append one spoken message to the lead conversation. Idempotency is provided
/// by a caller-supplied key (`voice:call:<n>`), so a retransmitted utterance
/// cannot double the transcript.
pub async fn append_conversation(
    pool: &PgPool,
    lead_id: LeadId,
    direction: ConversationDirection,
    body: &str,
    idempotency_key: &str,
) -> Result<Uuid, AppError> {
    let entry = ConversationEntry {
        id: uuid::Uuid::new_v4(),
        lead_id,
        email_id: None,
        direction,
        state: match direction {
            ConversationDirection::Outbound => OutboundState::Sent,
            ConversationDirection::Inbound => OutboundState::Approved,
        },
        subject: "звонок".to_string(),
        body: body.to_string(),
        idempotency_key: idempotency_key.to_string(),
        sent_at: None,
        created_at: chrono::Utc::now(),
    };
    conversation_repo::append(pool, &entry).await
}

/// Latest transcript for a lead, newest first.
pub async fn conversation(
    pool: &PgPool,
    lead_id: LeadId,
) -> Result<Vec<ConversationEntry>, AppError> {
    conversation_repo::history(pool, lead_id, 50).await
}

/// Current status of a voice lead. Missing leads error, so routes never guess.
pub async fn status_of(
    pool: &PgPool,
    lead_id: LeadId,
) -> Result<crate::domain::LeadStatus, AppError> {
    Ok(lead_repo::get(pool, lead_id).await?.status)
}
