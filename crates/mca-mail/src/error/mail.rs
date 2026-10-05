//! Failures of the mail gateway layer.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MailError {
    #[error("could not connect to mail server: {0}")]
    Connect(String),
    #[error("mail authentication failed: {0}")]
    Auth(String),
    #[error("mailbox unavailable: {0}")]
    Unavailable(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("mailbox folder not found: {0}")]
    NoSuchFolder(String),
    #[error("message too large: {0}")]
    TooLarge(String),
    #[error("attachment blocked: {0}")]
    BlockedAttachment(String),
    #[error("mail send rejected: {0}")]
    Rejected(String),
    /// A mailbox-mutating operation was refused by the read-only guard.
    ///
    /// Carries the operation name so the refusal is auditable, never the mail
    /// content or any credential.
    #[error("operation not allowed in {mode} mode: {op}")]
    OperationNotAllowed { op: String, mode: String },
}
