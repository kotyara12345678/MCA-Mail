use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::class::{EmailCategory, FieldState};
use super::ids::{ContactId, ConversationId, LeadId, ThreadId};
use super::requirement::RequirementField;
use super::requirement::{RequirementScope, RequirementSource};

crate::domain::wire_enum! {
    /// Lifecycle of a commercial lead.
    LeadStatus {
        New => "new",
        Processing => "processing",
        AwaitingCustomer => "awaiting_customer",
        Qualified => "qualified",
        NeedsHumanReview => "needs_human_review",
        HandedOff => "handed_off",
        InProgress => "in_progress",
        Won => "won",
        Lost => "lost",
        Closed => "closed",
    }
}

impl LeadStatus {
    /// While a lead is in one of these states the Email Communication Agent is
    /// forbidden from sending anything; only a manager can release it.
    pub const fn automation_locked(&self) -> bool {
        matches!(
            self,
            LeadStatus::HandedOff
                | LeadStatus::InProgress
                | LeadStatus::Won
                | LeadStatus::Lost
                | LeadStatus::Closed
                | LeadStatus::NeedsHumanReview
        )
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            LeadStatus::Won | LeadStatus::Lost | LeadStatus::Closed
        )
    }
}

crate::domain::wire_enum! {
    /// How a lead entered the system.
    LeadSource {
        InboundEmail => "inbound_email",
        Api => "api",
        Manager => "manager",
    }
}

/// A person or company the lead belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    pub id: ContactId,
    pub lead_id: LeadId,
    pub name: Option<String>,
    pub position: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub telegram: Option<String>,
    pub company_name: Option<String>,
    pub company_inn: Option<String>,
    pub is_primary: bool,
    pub created_at: DateTime<Utc>,
}

impl Contact {
    /// Short label for manager-facing lists.
    pub fn label(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.company_name.clone())
            .or_else(|| self.email.clone())
            .or_else(|| self.phone.clone())
            .unwrap_or_else(|| "неизвестный контакт".to_string())
    }
}

/// A commercial opportunity derived from the correspondence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lead {
    pub id: LeadId,
    pub thread_id: Option<ThreadId>,
    pub primary_email_id: Option<super::ids::EmailId>,
    pub source: LeadSource,
    pub status: LeadStatus,
    pub category: Option<EmailCategory>,
    pub scope: RequirementScope,
    /// Canonical conversation key: normalized customer address plus normalized
    /// subject. This is what makes reprocessing idempotent.
    pub conversation_key: String,
    pub contact_email: String,
    pub company_name: Option<String>,
    pub company_inn: Option<String>,
    pub summary: Option<String>,
    pub confidence: Option<f32>,
    /// Questions the agent still needs answered.
    pub open_questions: Vec<String>,
    pub unresolved_topics: Vec<String>,
    /// Set when the customer asked for a call, a specific manager, or terms.
    pub callback_requested: bool,
    pub callback_phone: Option<String>,
    pub automation_released_by: Option<ContactId>,
    pub last_activity_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Lead {
    /// All fields that still prevent MCA from quoting the request.
    pub fn blocking_gaps(
        requirements: &[super::requirement::LeadRequirement],
    ) -> Vec<RequirementField> {
        requirements
            .iter()
            .filter(|r| r.field.is_quote_blocking() && !r.state.is_trusted())
            .map(|r| r.field)
            .collect()
    }

    /// Values the customer stated, safe to show a manager as fact.
    pub fn confirmed_facts(
        requirements: &[super::requirement::LeadRequirement],
    ) -> Vec<(RequirementField, String)> {
        requirements
            .iter()
            .filter(|r| r.state == FieldState::Known && r.source == RequirementSource::Customer)
            .filter_map(|r| r.value.clone().map(|v| (r.field, v)))
            .collect()
    }

    /// Is it safe for the agent to reply automatically right now?
    pub fn reply_allowed(&self) -> bool {
        !self.status.automation_locked()
    }
}

crate::domain::wire_enum! {
    /// Per-message state inside a lead conversation.
    ConversationDirection {
        Inbound => "inbound",
        Outbound => "outbound",
    }
}

crate::domain::wire_enum! {
    /// Lifecycle of a generated reply.
    OutboundState {
        Draft => "draft",
        Approved => "approved",
        Sent => "sent",
        Rejected => "rejected",
        Suppressed => "suppressed",
    }
}

/// One entry of the conversation history kept for every lead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub id: ConversationId,
    pub lead_id: LeadId,
    pub email_id: Option<super::ids::EmailId>,
    pub direction: ConversationDirection,
    pub state: OutboundState,
    pub subject: String,
    pub body: String,
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// Idempotency key used to guarantee at-most-once delivery.
    pub idempotency_key: String,
}
