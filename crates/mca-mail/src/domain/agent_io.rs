use serde::{Deserialize, Serialize};

use super::class::{Confidence, FieldState};
use super::ids::{EmailId, LeadId};
use super::requirement::RequirementField;

/// Structured reply produced by the Lead Qualification Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualificationResult {
    pub lead_id: Option<LeadId>,
    /// Extracted parameters. Fields absent from the list stay `unknown`.
    pub requirements: Vec<ExtractedRequirement>,
    /// Summary of what the customer actually asked for.
    pub request_summary: String,
    /// Questions worth asking right now. Bounded by the agent loop.
    pub questions: Vec<String>,
    /// Service combination the agent identified.
    pub scope: super::requirement::RequirementScope,
    /// `0.0..=1.0` overall confidence in the extraction.
    pub confidence: Confidence,
    /// Topics requiring compliance review (regulated goods, sanctions scope).
    pub regulated_topics: Vec<String>,
    /// Facts the customer stated that the agent could not reconcile, e.g.
    /// contradictory weights between two messages.
    pub contradictions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractedRequirement {
    pub field: RequirementField,
    /// `null` means the agent explicitly found no value.
    pub value: Option<String>,
    pub state: FieldState,
    pub unit: Option<String>,
    pub confidence: Option<f32>,
    /// Verbatim span from the customer's own words that justifies the value.
    /// Retained so a manager can audit the extraction.
    pub evidence: Option<String>,
}

impl QualificationResult {
    pub fn get(&self, field: RequirementField) -> Option<&ExtractedRequirement> {
        self.requirements.iter().find(|r| r.field == field)
    }

    /// Values the agent claims were stated by the customer.
    pub fn confirmed(&self, field: RequirementField) -> Option<&str> {
        self.get(field)
            .filter(|r| r.state == FieldState::Known)
            .and_then(|r| r.value.as_deref())
    }

    /// Questions that have not been answered yet. A question already asked in a
    /// previous turn must not be repeated.
    pub fn unanswered(&self, asked_before: &[String]) -> Vec<String> {
        self.questions
            .iter()
            .filter(|q| {
                let needle = normalize_question(q);
                !asked_before.iter().any(|a| {
                    let prev = normalize_question(a);
                    needle == prev || needle.contains(&prev) || prev.contains(&needle)
                })
            })
            .cloned()
            .collect()
    }
}

/// Case- and punctuation-insensitive comparison key for questions.
pub fn normalize_question(question: &str) -> String {
    let lowered = question.to_lowercase();
    let filtered: String = lowered
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    filtered.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reply produced by the Email Communication Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommunicationPlan {
    pub subject: String,
    pub body: String,
    /// `draft` keeps the message for a human; `send` requests dispatch.
    pub disposition: ReplyDisposition,
    /// Set when the agent believes a manager should take over.
    pub handoff_requested: bool,
    pub handoff_reason: Option<super::handoff::HandoffReason>,
    pub rationale: String,
    pub confidence: Confidence,
}

crate::domain::wire_enum! {
    ReplyDisposition {
        /// Produce a draft for manual approval.
        Draft => "draft",
        /// Dispatch immediately; only honoured when policy allows.
        Send => "send",
        /// Say nothing at all (spam, duplicates, locked leads).
        Suppress => "suppress",
    }
}

impl ReplyDisposition {
    pub const fn is_send(&self) -> bool {
        matches!(self, ReplyDisposition::Send)
    }
}

/// Package produced by the Handoff Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffRequest {
    pub lead_id: LeadId,
    pub email_id: Option<EmailId>,
    pub reason: super::handoff::HandoffReason,
    pub priority: super::handoff::Priority,
    pub cargo_summary: String,
    pub route_summary: String,
    pub requested_service: String,
    pub missing_information: Vec<String>,
    pub open_questions: Vec<String>,
    pub conversation_digest: String,
    pub checks_performed: Vec<String>,
    pub unresolved_topics: Vec<String>,
    pub original_request: String,
    pub rationale: String,
}

/// Result of a single logistics-consultation turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogisticsAdvice {
    /// Subject-matter explanation safe to send to a customer.
    pub explanation: String,
    pub questions: Vec<String>,
    /// Claims the agent must not make, listed for the policy validator.
    pub forbidden_claims_avoided: Vec<String>,
    /// Topics that need a human specialist (customs code, sanctions scope).
    pub escalate_topics: Vec<String>,
    pub confidence: Confidence,
}

#[cfg(test)]
#[path = "agent_io_test.rs"]
mod tests;
