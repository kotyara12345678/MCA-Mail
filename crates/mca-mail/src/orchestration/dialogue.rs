//! The state of an ongoing email dialogue with one lead.
//!
//! Everything here is read from tables that already exist — `lead_conversations`
//! for what was said, `lead_requirements` for what is known, `leads.automation_locked`
//! for whether the system may speak at all. Nothing is kept in memory between
//! runs, which is what lets a restart resume a conversation instead of
//! starting a new one.

use sqlx::PgPool;

use crate::domain::{
    normalize_question, ConversationDirection, ConversationEntry, FieldState, LeadId,
    LeadRequirement, RequirementField,
};
use crate::error::AppError;
use crate::persistence::{conversation_repo, lead_repo, requirement_repo};

/// Everything the next turn needs, loaded once per email.
#[derive(Debug, Default)]
pub struct Dialogue {
    /// Bodies of previous outbound messages, newest first.
    pub asked: Vec<String>,
    /// Normalised form of [`Self::asked`], for repeat detection.
    pub asked_norm: Vec<String>,
    pub requirements: Vec<LeadRequirement>,
    /// Fields that still prevent MCA from quoting the request.
    pub missing: Vec<RequirementField>,
    /// Every field the customer has not given us yet. This, not
    /// [`Self::missing`], is what the reply agent is asked to collect: the
    /// quote-blocking subset only decides when the dialogue may end.
    pub open: Vec<RequirementField>,
    pub outbound_count: i64,
    /// Consecutive outbound messages with no customer reply in between.
    pub unanswered: i64,
    /// Handoff open, or a manager owns the conversation.
    pub locked: bool,
    /// Nothing quote-blocking is missing: the lead can go to a manager.
    pub complete: bool,
    pub history: Vec<ConversationEntry>,
}

impl Dialogue {
    pub async fn load(pool: &PgPool, lead_id: LeadId) -> Result<Self, AppError> {
        let asked = conversation_repo::asked_questions(pool, lead_id).await?;
        let history = conversation_repo::history(pool, lead_id, 20).await?;
        let requirements = requirement_repo::all(pool, lead_id).await?;
        let outbound_count = conversation_repo::outbound_count(pool, lead_id).await?;
        let unanswered = conversation_repo::unanswered_replies(pool, lead_id).await?;
        let locked = lead_repo::is_automation_locked(pool, lead_id).await?;

        let asked_norm = asked.iter().map(|q| normalize_question(q)).collect();
        let missing = crate::domain::Lead::blocking_gaps(&requirements);
        let open = crate::domain::Lead::open_gaps(&requirements);
        let complete = missing.is_empty();
        Ok(Dialogue {
            asked,
            asked_norm,
            requirements,
            missing,
            open,
            outbound_count,
            unanswered,
            locked,
            complete,
            history,
        })
    }

    /// Drop questions the customer has already been asked.
    pub fn new_questions(&self, candidates: &[String]) -> Vec<String> {
        candidates
            .iter()
            .filter(|q| {
                let needle = normalize_question(q);
                !needle.is_empty()
                    && !self.asked_norm.iter().any(|prev| {
                        prev == &needle || needle.contains(prev.as_str()) || prev.contains(&needle)
                    })
            })
            .cloned()
            .collect()
    }

    /// Facts the customer already gave, rendered for the prompt.
    pub fn known_facts(&self) -> Vec<String> {
        self.requirements
            .iter()
            .filter(|r| r.value.is_some() && r.state != FieldState::Unknown)
            .filter_map(|r| {
                r.value
                    .as_deref()
                    .map(|v| format!("{} = {}", r.field.as_str(), v))
            })
            .collect()
    }
}

/// Inbound idempotency key: replaying the same email cannot append twice.
pub(super) fn inbound_key(email_id: uuid::Uuid) -> String {
    format!("in:{email_id}")
}

/// Outbound idempotency key: one conversation entry per draft.
pub(super) fn outbound_key(draft_id: uuid::Uuid) -> String {
    crate::persistence::draft_repo::outbound_key(draft_id)
}

/// Append an inbound customer message to the lead's conversation.
///
/// The unique index on `idempotency_key` is the whole guarantee: a worker that
/// crashes after writing and is retried writes nothing the second time.
pub(super) async fn record_inbound(
    pool: &PgPool,
    entry: &ConversationEntry,
) -> Result<(), AppError> {
    debug_assert_eq!(entry.direction, ConversationDirection::Inbound);
    conversation_repo::append(pool, entry).await?;
    Ok(())
}

/// Append an outbound reply to the lead's conversation.
pub(super) async fn record_outbound(
    pool: &PgPool,
    entry: &ConversationEntry,
) -> Result<(), AppError> {
    debug_assert_eq!(entry.direction, ConversationDirection::Outbound);
    conversation_repo::append(pool, entry).await?;
    Ok(())
}
