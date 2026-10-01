use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ids::{EmailId, LeadId, RunId, ThreadId};

crate::domain::wire_enum! {
    RunState {
        Queued => "queued",
        Running => "running",
        Succeeded => "succeeded",
        Failed => "failed",
        Cancelled => "cancelled",
        /// Failed but scheduled for automatic retry.
        RetryScheduled => "retry_scheduled",
    }
}

impl RunState {
    pub const fn is_terminal(&self) -> bool {
        !matches!(
            self,
            RunState::Queued | RunState::Running | RunState::RetryScheduled
        )
    }
}

crate::domain::wire_enum! {
    /// One processing step inside a run. Persisted so a crash mid-run can be
    /// resumed without repeating completed, side-effecting steps.
    ProcessingStage {
        Intake => "intake",
        TechnicalPrefilter => "technical_prefilter",
        Spam => "spam",
        Classification => "classification",
        Deduplication => "deduplication",
        LeadResolution => "lead_resolution",
        Qualification => "qualification",
        CompanyResearch => "company_research",
        ReplyPlanning => "reply_planning",
        Handoff => "handoff",
        Delivery => "delivery",
        Finalize => "finalize",
    }
}

crate::domain::wire_enum! {
    EventSeverity {
        Debug => "debug",
        Info => "info",
        Warning => "warning",
        Error => "error",
    }
}

/// An inbound email thread, keyed by RFC 5322 references when available.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmailThread {
    pub id: ThreadId,
    pub root_message_id: Option<String>,
    pub normalized_subject: String,
    pub conversation_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The unit of work: process exactly one inbound email end to end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessingRun {
    pub id: RunId,
    pub email_id: EmailId,
    pub thread_id: ThreadId,
    pub lead_id: Option<LeadId>,
    pub state: RunState,
    /// Incremented on every manual or automatic reprocess of the same email.
    pub attempt: i32,
    pub trigger: RunTrigger,
    pub current_stage: Option<ProcessingStage>,
    pub stages_completed: Vec<ProcessingStage>,
    pub error: Option<String>,
    pub total_tokens: i64,
    pub cost_micros_usd: i64,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

crate::domain::wire_enum! {
    RunTrigger {
        Poll => "poll",
        Manual => "manual",
        Api => "api",
        Retry => "retry",
    }
}

/// A single agent invocation inside a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub id: super::ids::AgentRunId,
    pub run_id: RunId,
    pub email_id: EmailId,
    pub agent: super::email_record::AgentKind,
    pub model: String,
    pub state: RunState,
    pub iterations: i32,
    pub tool_calls: i32,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_micros_usd: i64,
    pub duration_ms: i64,
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

crate::domain::wire_enum! {
    ToolCallStatus {
        Started => "started",
        Succeeded => "succeeded",
        Failed => "failed",
        Denied => "denied",
        TimedOut => "timed_out",
    }
}

/// Audit record of a tool invocation. Arguments are redacted before storage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: super::ids::ToolCallId,
    pub run_id: RunId,
    pub agent_run_id: super::ids::AgentRunId,
    pub email_id: EmailId,
    pub agent: super::email_record::AgentKind,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub status: ToolCallStatus,
    pub result_preview: Option<String>,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub created_at: DateTime<Utc>,
}

/// Append-only operational event stream. Used for debugging, audit and the
/// admin API; never contains raw email bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessingEvent {
    pub id: super::ids::EventId,
    pub run_id: Option<RunId>,
    pub email_id: Option<EmailId>,
    pub lead_id: Option<LeadId>,
    pub stage: Option<ProcessingStage>,
    pub severity: EventSeverity,
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
    pub created_at: DateTime<Utc>,
}
