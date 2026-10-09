//! The order card queued when a phone call finishes.
//!
//! One function decides everything server-side: may this lead receive a card
//! (`plan`), and push it into the same outbox every other outbound email uses
//! (`enqueue`). The send worker's policy check — mode, auto_send, automation
//! lock, quotas — remains the only gate that can actually deliver it, so
//! queuing here never means "sent". A skip is never silent: the reason comes
//! back to the caller and is written to the audit log.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::domain::{Lead, LeadId, LeadRequirement, OrderCard};
use crate::error::AppError;
use crate::persistence::{
    lead_repo, outbox_repo,
    outbox_repo::{OutboundIntent, OutboundKind},
    requirement_repo,
};
use crate::voice;

/// Why no card could be built from what the call left behind. Every variant
/// carries enough detail for the caller (and the audit log) to say what a
/// human has to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// No usable address: `leads.contact_email` still holds the caller
    /// number because the customer never dictated an email.
    MissingEmail,
    /// Quote-blocking requirements are still unanswered; the card would be a
    /// form without the order on it.
    Incomplete { missing: Vec<&'static str> },
}

impl SkipReason {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::MissingEmail => "missing_email",
            Self::Incomplete { .. } => "incomplete_requirements",
        }
    }

    /// Wire names of the blocking fields, empty for `MissingEmail`.
    pub fn missing(&self) -> &[&'static str] {
        match self {
            Self::MissingEmail => &[],
            Self::Incomplete { missing } => missing,
        }
    }
}

/// Everything queued, decided purely from database state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderCardPlan {
    pub recipient: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: String,
}

/// Result of an order-card attempt for one finished call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderCardOutcome {
    Queued { outbox_id: Uuid },
    AlreadyQueued,
    Skipped { reason: SkipReason },
}

impl OrderCardOutcome {
    /// Wire state for the finish-call response.
    pub const fn state(&self) -> &'static str {
        match self {
            Self::Queued { .. } => "queued",
            Self::AlreadyQueued => "already_queued",
            Self::Skipped { .. } => "skipped",
        }
    }
}

/// Idempotency key: one card per lead per recipient, ever. The address is
/// part of the key exactly as it is for the manager card — normalize once
/// here and a re-dictated email can never produce a second row.
pub fn order_card_key(lead_id: LeadId, recipient: &str) -> String {
    format!(
        "order_card:{lead_id}:{}",
        recipient.trim().to_ascii_lowercase()
    )
}

/// Decide — without touching the database — whether this lead may receive a
/// card, and build it from what is already stored.
///
/// Nothing is invented: the card renders whatever the requirements hold and
/// prints `не предоставлено` for the rest. The two gates are a usable email
/// address and zero quote-blocking gaps; each failure returns its reason so
/// the caller can record *why* no card exists.
pub fn plan(
    lead: &Lead,
    requirements: &[LeadRequirement],
    now: DateTime<Utc>,
) -> Result<OrderCardPlan, SkipReason> {
    let recipient = lead.contact_email.trim().to_ascii_lowercase();
    if !voice::is_email(&recipient) {
        return Err(SkipReason::MissingEmail);
    }
    let blocking = Lead::blocking_gaps(requirements);
    if !blocking.is_empty() {
        return Err(SkipReason::Incomplete {
            missing: blocking.iter().map(|f| f.as_str()).collect(),
        });
    }
    let card = OrderCard {
        lead,
        requirements,
        generated_at: now,
    };
    Ok(OrderCardPlan {
        recipient,
        subject: card.subject(),
        body_text: card.to_text(),
        body_html: card.to_html(),
    })
}

/// Build the card for `lead_id` and queue it, once per recipient.
///
/// Idempotent by construction: a replayed `finish_call` lands on the same
/// idempotency key (and the same partial unique index) and reports
/// `already_queued` instead of a second email.
pub async fn enqueue(pool: &PgPool, lead_id: LeadId) -> Result<OrderCardOutcome, AppError> {
    let lead = lead_repo::get(pool, lead_id).await?;
    let requirements = requirement_repo::all(pool, lead_id).await?;
    let plan = match plan(&lead, &requirements, Utc::now()) {
        Ok(plan) => plan,
        Err(reason) => return Ok(OrderCardOutcome::Skipped { reason }),
    };
    let intent = OutboundIntent {
        message_type: OutboundKind::OrderCard,
        lead_id: Some(lead_id),
        email_id: lead.primary_email_id,
        run_id: None,
        recipient: plan.recipient.clone(),
        subject: plan.subject,
        body_text: plan.body_text,
        body_html: plan.body_html,
        in_reply_to: None,
        ref_headers: Vec::new(),
        idempotency_key: order_card_key(lead_id, &plan.recipient),
        correlation_id: None,
    };
    match outbox_repo::enqueue_send(pool, &intent).await? {
        Some(outbox_id) => {
            info!(
                outbox_id = %outbox_id,
                lead_id = %lead_id,
                recipient = %intent.recipient,
                "order card queued"
            );
            Ok(OrderCardOutcome::Queued { outbox_id })
        }
        None => {
            info!(lead_id = %lead_id, "order card already queued for this lead");
            Ok(OrderCardOutcome::AlreadyQueued)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        FieldState, LeadSource, RequirementField, RequirementScope, RequirementSource,
    };

    fn lead(email: &str) -> Lead {
        let now = Utc::now();
        Lead {
            id: Uuid::nil(),
            thread_id: None,
            primary_email_id: None,
            source: LeadSource::Api,
            status: crate::domain::LeadStatus::Qualified,
            category: None,
            scope: RequirementScope::Transport,
            conversation_key: "voice:79161234567".into(),
            contact_email: email.to_string(),
            company_name: None,
            company_inn: None,
            summary: None,
            confidence: None,
            open_questions: vec![],
            unresolved_topics: vec![],
            callback_requested: false,
            callback_phone: None,
            automation_released_by: None,
            last_activity_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    fn blocking() -> Vec<LeadRequirement> {
        // GoodsName and OriginCountry are quote-blocking and unanswered.
        [
            RequirementField::GoodsName,
            RequirementField::OriginCountry,
            RequirementField::DestinationCountry,
            RequirementField::GoodsWeight,
        ]
        .into_iter()
        .map(|field| LeadRequirement {
            lead_id: Uuid::nil(),
            field,
            value: None,
            state: FieldState::Unknown,
            source: RequirementSource::Customer,
            unit: None,
            confidence: None,
            evidence: None,
            updated_at: Utc::now(),
        })
        .collect()
    }

    fn answered() -> Vec<LeadRequirement> {
        blocking()
            .into_iter()
            .map(|mut r| {
                r.value = Some("12".to_string());
                r.state = FieldState::Known;
                r
            })
            .collect()
    }

    #[test]
    fn caller_number_is_not_an_email() {
        // ensure_voice_lead stores the phone in contact_email until the
        // customer dictates a real address.
        let err = plan(&lead("79161234567"), &answered(), Utc::now()).unwrap_err();
        assert_eq!(err, SkipReason::MissingEmail);
        assert_eq!(err.as_str(), "missing_email");
        assert!(err.missing().is_empty());
    }

    #[test]
    fn blocking_requirements_are_a_reason_not_a_half_written_card() {
        let err = plan(&lead("a@b.test"), &blocking(), Utc::now()).unwrap_err();
        assert_eq!(err.as_str(), "incomplete_requirements");
        assert_eq!(
            err.missing(),
            [
                "goods_name",
                "origin_country",
                "destination_country",
                "goods_weight"
            ]
        );
    }

    #[test]
    fn a_complete_lead_with_an_address_produces_a_card() {
        let mut l = lead("  Buyer@Acme.TEST ");
        l.summary = Some("Перевозка запчастей".into());
        let p = plan(&l, &answered(), Utc::now()).expect("plan");
        // Recipient is normalized so the idempotency key is stable.
        assert_eq!(p.recipient, "buyer@acme.test");
        assert!(!p.subject.is_empty());
        assert!(p.body_text.contains("КАРТОЧКА ЗАКАЗА"));
        assert!(p.body_text.contains("buyer@acme.test"));
        assert!(p.body_html.contains("<!doctype html>"));
    }

    #[test]
    fn key_is_case_insensitive_and_pinned_to_the_recipient() {
        let id = Uuid::nil();
        assert_eq!(
            order_card_key(id, "Buyer@Acme.test"),
            order_card_key(id, " buyer@acme.test ")
        );
        assert_ne!(
            order_card_key(id, "a@x.test"),
            order_card_key(id, "b@x.test")
        );
        assert!(order_card_key(id, "a@x.test").starts_with(&format!("order_card:{id}:")));
    }
}
