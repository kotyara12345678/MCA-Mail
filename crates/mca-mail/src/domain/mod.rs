//! Domain layer: pure data, invariants and traits. No I/O, no framework types.

mod address;
mod agent_io;
mod channel;
mod class;
mod classification;
mod draft;
mod email_record;
mod handoff;
mod ids;
mod lead;
mod message;
mod processing;
mod requirement;
mod research;
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
pub use message::{
    EmailAttachment, ExtractionStatus, InboundMessage, MailboxIdentity, MessagePart,
    OutboundAttachment, OutboundMessage,
};
pub use processing::{
    AgentRunRecord, EmailThread, EventSeverity, ProcessingEvent, ProcessingRun, ProcessingStage,
    RunState, RunTrigger, ToolCallRecord, ToolCallStatus,
};
pub use requirement::{LeadRequirement, RequirementField, RequirementScope, RequirementSource};
pub use research::{
    CompanyIdentifiers, CompanyResearchReport, CompanyStatus, ResearchSource, ResearchStatus,
};
pub(crate) use wire_enum::wire_enum;
