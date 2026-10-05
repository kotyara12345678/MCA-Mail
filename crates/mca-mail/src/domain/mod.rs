//! Domain layer: pure data, invariants and traits. No I/O, no framework types.

mod address;
mod agent_io;
mod channel;
mod class;
mod classification;
mod draft;
mod email_record;
pub(crate) mod flex;
mod handoff;
mod ids;
mod lead;
mod manager_card;
mod message;
mod processing;
mod requirement;
mod research;
mod spam_policy;
mod wire_enum;

pub use address::{normalize_subject, EmailAddress};
pub use agent_io::{
    normalize_question, CommunicationPlan, ExtractedRequirement, HandoffRequest, LogisticsAdvice,
    QualificationResult, ReplyDisposition,
};
pub use channel::{
    ChannelError, ChannelHealth, ChannelMetadata, CommunicationChannel, ContactRef, Conversation,
    InboundEnvelope, OutboundEnvelope,
};
pub use class::{Confidence, EmailCategory, FieldState, SpamVerdict};
pub use classification::{ClassificationOutcome, ClassificationRecord, NextAction, SpamAssessment};
pub use draft::{DraftStatus, EmailDraft};
pub use email_record::{AgentKind, Direction, EmailStatus, MailboxAction, ModelTier, StoredEmail};
pub use handoff::{Handoff, HandoffReason, HandoffState, Priority};
pub use ids::{
    AgentRunId, ApiKeyId, AttachmentId, AuditId, ContactId, ConversationId, CostMicrosUsd, DraftId,
    EmailId, EventId, HandoffId, LeadId, RequirementId, ResearchId, RunId, ThreadId, TokenCount,
    ToolCallId, TraceId, WireParseError,
};
pub use lead::{
    Contact, ConversationDirection, ConversationEntry, Lead, LeadSource, LeadStatus, OutboundState,
};
pub use manager_card::{ManagerCard, UNKNOWN as MANAGER_CARD_UNKNOWN};
pub use message::{
    EmailAttachment, ExtractionStatus, InboundMessage, MailboxIdentity, MessagePart,
    OutboundAttachment, OutboundMessage,
};
pub use processing::{
    AgentRunRecord, EmailThread, EventSeverity, ProcessingEvent, ProcessingRun, ProcessingStage,
    RunState, RunTrigger, ToolCallRecord, ToolCallStatus,
};
pub use requirement::{
    normalize, LeadRequirement, NormalizeReport, RawRequirement, RequirementField,
    RequirementScope, RequirementSource,
};
pub use research::{
    CompanyIdentifiers, CompanyResearchReport, CompanyStatus, ResearchSource, ResearchStatus,
};
pub use spam_policy::confident_spam;
pub(crate) use wire_enum::wire_enum;
