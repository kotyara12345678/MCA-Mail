use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::class::{EmailCategory, SpamVerdict};
use super::ids::{EmailId, HandoffId, LeadId, RunId, ThreadId};

crate::domain::wire_enum! {
    /// Why the AI decided a person must take over.
    HandoffReason {
        /// Customer asked for a call, a specific manager, or concrete terms.
        CallbackRequested => "callback_requested",
        /// Enough information collected; a human should run the commercial part.
        ReadyForManager => "ready_for_manager",
        /// Price, tariff, or legal terms were requested.
        PricingOrLegal => "pricing_or_legal",
        /// The goods may be regulated, sanctioned, or export-controlled.
        ComplianceReview => "compliance_review",
        /// Complaint or negative feedback.
        Complaint => "complaint",
        /// The model could not reach a confident answer.
        LowConfidence => "low_confidence",
        /// Suspicious or phishing-like message.
        SecurityConcern => "security_concern",
        /// An operator requested escalation through the API.
        ManualRequest => "manual_request",
    }
}

impl HandoffReason {
    pub const fn blocks_automation(&self) -> bool {
        true
    }
}

crate::domain::wire_enum! {
    /// Manager-facing triage priority.
    Priority {
        Low => "low",
        Normal => "normal",
        High => "high",
        Critical => "critical",
    }
}

impl Priority {
    /// Stored as a small integer so SQL can order and compare priority directly.
    pub const fn as_i32(&self) -> i32 {
        match self {
            Priority::Low => 10,
            Priority::Normal => 20,
            Priority::High => 30,
            Priority::Critical => 40,
        }
    }

    /// Reverse of [`Priority::as_i32`]; unknown ranks degrade to `normal`.
    pub fn from_i32(value: i32) -> Self {
        match value {
            10 => Priority::Low,
            30 => Priority::High,
            40 => Priority::Critical,
            _ => Priority::Normal,
        }
    }
}

crate::domain::wire_enum! {
    HandoffState {
        Open => "open",
        Acknowledged => "acknowledged",
        Resolved => "resolved",
        ReturnedToAutomation => "returned_to_automation",
    }
}

/// A structured package handed to a human manager.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handoff {
    pub id: HandoffId,
    pub lead_id: LeadId,
    pub thread_id: Option<ThreadId>,
    pub run_id: Option<RunId>,
    /// Message that triggered the handoff, so a manager opens the original.
    pub email_id: Option<EmailId>,
    pub reason: HandoffReason,
    pub priority: Priority,
    pub state: HandoffState,
    /// Contact block rendered for the manager.
    pub contact_email: String,
    pub contact_name: Option<String>,
    pub contact_phone: Option<String>,
    pub company_name: Option<String>,
    pub company_inn: Option<String>,
    /// Verbatim original request, retained because a manager needs the source.
    pub original_request: String,
    pub category: Option<EmailCategory>,
    pub spam_verdict: Option<SpamVerdict>,
    /// Extracted cargo characteristics rendered as `field: value` lines.
    pub cargo_summary: String,
    pub route_summary: String,
    pub requested_service: String,
    pub missing_information: Vec<String>,
    pub open_questions: Vec<String>,
    pub conversation_digest: String,
    pub research_digest: Option<String>,
    pub checks_performed: Vec<String>,
    /// Unresolved topics, e.g. `customs_classification`, `sanctions_scope`.
    pub unresolved_topics: Vec<String>,
    pub assigned_to: Option<String>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Handoff {
    /// One-line summary used in logs and manager notifications.
    pub fn headline(&self) -> String {
        format!(
            "{} | {} | {}",
            self.priority.as_str(),
            self.reason.as_str(),
            self.company_name.as_deref().unwrap_or(&self.contact_email)
        )
    }
}
