//! Security policy denials.
//!
//! Each variant is separately testable and separately auditable, because a
//! denial is a security-relevant event: the audit trail must be able to say
//! *which* rule fired, not merely that "something was refused".

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("mode `{mode}` forbids outbound mail actions")]
    ModeForbidsSending { mode: String },
    #[error("hourly send limit of {limit} reached")]
    SendRateLimit { limit: u32 },
    #[error("automation is locked for lead {lead_id}: {reason}")]
    AutomationLocked { lead_id: String, reason: String },
    #[error("outbound content rejected: {reason}")]
    ContentRejected { reason: String },
    #[error("disallowed claim detected in outbound content: {claim}")]
    DisallowedClaim { claim: String },
    #[error("attachment blocked: {reason}")]
    AttachmentBlocked { reason: String },
    #[error("destructive mail operation `{operation}` is not permitted")]
    DestructiveOperation { operation: String },
    /// A guard rail refused an action. `reason` is the stable
    /// [`crate::mail::PolicyDenial::as_str`] code recorded on the draft.
    #[error("outbound denied by policy: {reason}")]
    Denied { reason: String },
    #[error("prompt injection indicators were neutralised: {indicators}")]
    InjectionBlocked { indicators: String },
    #[error("cross-tenant data access denied: {context}")]
    CrossLeadAccess { context: String },
}
