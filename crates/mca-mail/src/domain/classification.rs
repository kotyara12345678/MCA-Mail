use serde::{Deserialize, Serialize};

use super::class::{Confidence, EmailCategory, SpamVerdict};
use super::ids::{EmailId, RunId};

/// Structured verdict produced by the Spam Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpamAssessment {
    pub verdict: SpamVerdict,
    pub confidence: Confidence,
    /// Short, loggable justification. Never contains the message body.
    pub rationale: String,
    /// Concrete suspicious signals, e.g. `credential_request`, `lookalike_domain`.
    pub signals: Vec<String>,
    /// Set when a human must inspect the message before further processing.
    pub requires_human_review: bool,
}

/// Structured verdict produced by the Classification Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassificationOutcome {
    pub category: EmailCategory,
    pub confidence: Confidence,
    /// One-sentence explanation retained for the audit trail.
    pub rationale: String,
    /// What the orchestrator is expected to do next.
    pub next_actions: Vec<NextAction>,
}

crate::domain::wire_enum! {
    /// Actions the Classification Agent may request from the orchestrator.
    NextAction {
        ExtractRequirements => "extract_requirements",
        DraftReply => "draft_reply",
        SendReply => "send_reply",
        ResearchCompany => "research_company",
        HandoffToManager => "handoff_to_manager",
        Quarantine => "quarantine",
        HumanReview => "human_review",
        None => "none",
    }
}

impl ClassificationOutcome {
    pub fn wants(&self, action: NextAction) -> bool {
        self.next_actions.contains(&action)
    }
}

/// Full persisted record of one classification attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassificationRecord {
    pub run_id: RunId,
    pub email_id: EmailId,
    pub category: EmailCategory,
    pub confidence: Confidence,
    pub rationale: String,
    pub next_actions: Vec<NextAction>,
    pub spam: Option<SpamAssessment>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
