//! Handoff assembly for a voice lead.
//!
//! Reuses the same `human_handoffs` table the email pipeline writes to, so a
//! manager sees one queue regardless of the channel. The manager destination
//! is never taken from the caller verbatim: only allowlisted keys are accepted,
//! and any other string is rejected before it reaches a dial plan.

use chrono::Utc;

use crate::domain::{Handoff, HandoffReason, Lead, LeadRequirement, Priority, RequirementField};
use crate::error::AppError;

/// Manager destinations the voice channel may request. Kept as an explicit
/// allowlist so a hallucinated extension can never reach the dialer.
pub const MANAGER_KEY_ALLOWLIST: &[&str] = &["mca_manager"];

/// Whether a manager key submitted by the call agent is allowed.
pub fn is_allowed_manager_key(key: &str) -> bool {
    MANAGER_KEY_ALLOWLIST.contains(&key)
}

/// Reject keys outside the allowlist with a stable error.
pub fn ensure_manager_key(key: &str) -> Result<(), AppError> {
    if is_allowed_manager_key(key) {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "manager destination `{key}` is not in the allowlist"
        )))
    }
}

/// Assemble the handoff a manager will open, reusing confirmed requirements.
///
/// `conversation` is the newest-first transcript from `lead_conversations`;
/// a digest is rendered so the manager sees the customer's own words.
pub fn build_handoff(
    lead: &Lead,
    requirements: &[LeadRequirement],
    conversation: &[crate::domain::ConversationEntry],
    reason: HandoffReason,
    priority: Priority,
    original_request: &str,
) -> Handoff {
    let value = |field: RequirementField| {
        requirements
            .iter()
            .find(|r| r.field == field && r.value.is_some())
            .and_then(|r| r.value.clone())
    };
    let digest = render_digest(conversation);

    Handoff {
        id: uuid::Uuid::new_v4(),
        lead_id: lead.id,
        thread_id: None,
        run_id: None,
        email_id: None,
        reason,
        priority,
        state: crate::domain::HandoffState::Open,
        contact_email: lead.contact_email.clone(),
        contact_name: value(RequirementField::ContactName),
        contact_phone: lead
            .callback_phone
            .clone()
            .or(value(RequirementField::ContactPhone)),
        company_name: lead
            .company_name
            .clone()
            .or(value(RequirementField::CompanyName)),
        company_inn: lead
            .company_inn
            .clone()
            .or(value(RequirementField::CompanyInn)),
        original_request: original_request.to_string(),
        category: None,
        spam_verdict: None,
        cargo_summary: cargo_summary(requirements),
        route_summary: route_summary(requirements),
        requested_service: lead.scope.as_str().to_string(),
        missing_information: missing_labels(requirements),
        open_questions: lead.open_questions.clone(),
        conversation_digest: digest,
        research_digest: None,
        checks_performed: Vec::new(),
        unresolved_topics: lead.unresolved_topics.clone(),
        assigned_to: None,
        acknowledged_at: None,
        created_at: Utc::now(),
    }
}

/// Render the customer's stated cargo facts as `label: value` lines.
fn cargo_summary(requirements: &[LeadRequirement]) -> String {
    use RequirementField as F;
    let mut facts: Vec<String> = requirements
        .iter()
        .filter(|r| r.state.is_trusted() && r.value.is_some())
        .filter(|r| {
            matches!(
                r.field,
                F::GoodsName
                    | F::GoodsDescription
                    | F::GoodsQuantity
                    | F::GoodsWeight
                    | F::GoodsVolume
                    | F::PackageCount
                    | F::GoodsValue
            )
        })
        .map(|r| {
            format!(
                "{}: {}",
                crate::voice::fields::label(r.field),
                r.value.as_deref().unwrap_or("")
            )
        })
        .collect();
    facts.sort();
    facts.join("; ")
}

/// Render origin → destination when both are known.
fn route_summary(requirements: &[LeadRequirement]) -> String {
    use RequirementField as F;
    let value = |field: F| {
        requirements
            .iter()
            .find(|r| r.field == field && r.value.is_some())
            .and_then(|r| r.value.clone())
            .unwrap_or_default()
    };
    let origin = value(F::OriginCountry);
    let destination = value(F::DestinationCountry);
    if origin.is_empty() && destination.is_empty() {
        String::new()
    } else {
        format!("{origin} → {destination}")
    }
}

/// Russian labels for everything still missing, for the manager at a glance.
fn missing_labels(requirements: &[LeadRequirement]) -> Vec<String> {
    Lead::open_gaps(requirements)
        .into_iter()
        .map(|field| crate::voice::fields::label(field).to_string())
        .collect()
}

/// Compact transcript digest (newest last) from newest-first input.
fn render_digest(conversation: &[crate::domain::ConversationEntry]) -> String {
    let lines: Vec<String> = conversation
        .iter()
        .rev()
        .take(12)
        .map(|entry| {
            let who = match entry.direction {
                crate::domain::ConversationDirection::Inbound => "Клиент",
                crate::domain::ConversationDirection::Outbound => "Агент",
            };
            format!("{who}: {}", entry.body)
        })
        .collect();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ConversationDirection, ConversationEntry, LeadRequirement, OutboundState, RequirementScope,
    };

    fn lead() -> Lead {
        Lead {
            id: uuid::Uuid::new_v4(),
            thread_id: None,
            primary_email_id: None,
            source: crate::domain::LeadSource::Api,
            status: crate::domain::LeadStatus::New,
            category: None,
            scope: RequirementScope::FullImport,
            conversation_key: "voice:79161234567".into(),
            contact_email: "79161234567".into(),
            company_name: None,
            company_inn: None,
            summary: None,
            confidence: None,
            open_questions: vec![],
            unresolved_topics: vec![],
            callback_requested: false,
            callback_phone: Some("79161234567".into()),
            automation_released_by: None,
            last_activity_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn only_allowlisted_manager_keys_accepted() {
        assert!(is_allowed_manager_key("mca_manager"));
        assert!(!is_allowed_manager_key("sip:9999"));
        assert!(!is_allowed_manager_key("anything"));
        assert!(ensure_manager_key("mca_manager").is_ok());
        assert!(ensure_manager_key("9999").is_err());
    }

    #[test]
    fn build_handoff_carries_confirmed_facts_as_digest() {
        let mut lead = lead();
        lead.company_name = Some("ООО Ромашка".into());
        let requirements = vec![
            LeadRequirement::stated(lead.id, RequirementField::GoodsName, "станки"),
            LeadRequirement::stated(lead.id, RequirementField::GoodsWeight, "120"),
            LeadRequirement::stated(lead.id, RequirementField::OriginCountry, "Китай"),
            LeadRequirement::stated(lead.id, RequirementField::DestinationCountry, "Россия"),
        ];
        let conversation = vec![ConversationEntry {
            id: uuid::Uuid::new_v4(),
            lead_id: lead.id,
            email_id: None,
            direction: ConversationDirection::Outbound,
            state: OutboundState::Sent,
            subject: "звонок".into(),
            body: "Здравствуйте, как мы можем помочь?".into(),
            idempotency_key: "voice:c:0".into(),
            sent_at: None,
            created_at: chrono::Utc::now(),
        }];

        let handoff = build_handoff(
            &lead,
            &requirements,
            &conversation,
            HandoffReason::ReadyForManager,
            Priority::High,
            "нужен расчёт перевозки",
        );

        assert_eq!(handoff.company_name.as_deref(), Some("ООО Ромашка"));
        assert!(handoff.cargo_summary.contains("станки"));
        assert!(handoff.route_summary.contains("Китай"));
        assert!(handoff.route_summary.contains("Россия"));
        assert_eq!(handoff.requested_service, "full_import");
        assert!(handoff.conversation_digest.contains("Здравствуйте"));
    }
}
