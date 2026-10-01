use serde::{Deserialize, Serialize};

/// Stable identity aliases.
///
/// Every persisted entity uses a UUIDv4 primary key. Provider-side identifiers
/// (IMAP `UID` + `UIDVALIDITY`, SMTP `Message-ID`) are stored in dedicated
/// columns and never used as primary keys, so a mailbox migration or provider
/// switch cannot corrupt references.
pub type EmailId = uuid::Uuid;
pub type ThreadId = uuid::Uuid;
pub type AttachmentId = uuid::Uuid;
pub type RunId = uuid::Uuid;
pub type AgentRunId = uuid::Uuid;
pub type ToolCallId = uuid::Uuid;
pub type EventId = uuid::Uuid;
pub type LeadId = uuid::Uuid;
pub type ContactId = uuid::Uuid;
pub type RequirementId = uuid::Uuid;
pub type ConversationId = uuid::Uuid;
pub type ResearchId = uuid::Uuid;
pub type HandoffId = uuid::Uuid;
pub type DraftId = uuid::Uuid;
pub type AuditId = uuid::Uuid;
pub type ApiKeyId = uuid::Uuid;
pub type TraceId = uuid::Uuid;

pub type TokenCount = i64;
pub type CostMicrosUsd = i64;

/// Failure to interpret a stored `TEXT` value as a known enum variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("`{value}` is not a valid {type_name}")]
pub struct WireParseError {
    pub type_name: &'static str,
    pub value: String,
}
