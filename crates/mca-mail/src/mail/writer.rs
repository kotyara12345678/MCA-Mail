//! MailProvider plus every operation that can change mailbox state.
//!
//! Reads live on [`MailProvider`]; anything that mutates the server lives here
//! with a default implementation that goes through the read-only guard. Splitting
//! the trait this way means a caller holding `&dyn MailProvider` cannot reach a
//! mutation, and a new provider cannot accidentally ship one without either
//! inheriting the guard or explicitly overriding it.

use async_trait::async_trait;

use crate::domain::OutboundMessage;
use crate::error::MailError;
use crate::mail::guard::MailGuard;
use crate::mail::ops::MailboxOp;

#[async_trait]
pub trait MailboxWriter: Send + Sync {
    /// The guard every mutating operation consults.
    fn guard(&self) -> MailGuard;

    /// Deliver a message over SMTP.
    async fn send(&self, _message: &OutboundMessage) -> Result<String, MailError> {
        Err(self.denied(MailboxOp::Send))
    }

    /// `APPEND` a draft to the server drafts folder.
    async fn append_draft(&self, _message: &OutboundMessage) -> Result<(), MailError> {
        Err(self.denied(MailboxOp::AppendDraft))
    }

    /// `APPEND` a message that has already been delivered to the sent folder.
    ///
    /// Separate from [`Self::send`] on purpose: SMTP delivery and keeping a
    /// copy are two operations with two outcomes. Archiving happens *after*
    /// the message is gone, so a failure here is reported by the caller as a
    /// warning and never as a delivery failure — that would make the next
    /// attempt send the same message twice.
    async fn append_sent(&self, _message: &OutboundMessage) -> Result<(), MailError> {
        Err(self.denied(MailboxOp::AppendSent))
    }

    /// `UID MOVE` to a server-discovered role, never an agent-supplied name.
    async fn move_to_role(
        &self,
        _uid: u32,
        _role: crate::mail::folders::FolderRole,
    ) -> Result<(), MailError> {
        Err(self.denied(MailboxOp::Move))
    }

    /// `UID COPY` to a server-discovered role.
    async fn copy_to_role(
        &self,
        _uid: u32,
        _role: crate::mail::folders::FolderRole,
    ) -> Result<(), MailError> {
        Err(self.denied(MailboxOp::Copy))
    }

    /// Permanently disabled; messages must be moved to the Trash role instead.
    async fn delete_message(&self, _uid: u32) -> Result<(), MailError> {
        Err(self.denied(MailboxOp::Delete))
    }

    /// Add a flag, e.g. `\Seen` or `$Junk`.
    async fn set_flag(&self, _uid: u32, _flag: &str) -> Result<(), MailError> {
        Err(self.denied(MailboxOp::SetFlag))
    }

    /// Remove a flag, e.g. `-FLAGS (\Seen)`.
    async fn clear_flag(&self, _uid: u32, _flag: &str) -> Result<(), MailError> {
        Err(self.denied(MailboxOp::ClearFlag))
    }

    /// Move a message into the configured quarantine folder.
    async fn quarantine(&self, _uid: u32, _reason: &str) -> Result<(), MailError> {
        Err(self.denied(MailboxOp::Move))
    }

    /// Record that a message's row was committed.
    async fn mark_processed(&self, _uid: u32) -> Result<(), MailError> {
        Err(self.denied(MailboxOp::SetFlag))
    }

    /// Close the session politely on shutdown.
    async fn close(&self) -> Result<(), MailError> {
        Ok(())
    }

    /// Build the refusal for an operation this provider does not implement.
    ///
    /// Uses the same variant as a mode refusal, because in both cases the
    /// guarantee that matters is identical: no network request was made. The
    /// guard is consulted first so read-only is logged as a policy refusal.
    fn denied(&self, op: MailboxOp) -> MailError {
        let guard = self.guard();
        if let Err(error) = guard.check(op, None) {
            return error;
        }
        MailError::OperationNotAllowed {
            op: op.as_str().to_string(),
            mode: guard.mode().as_str().to_string(),
        }
    }
}
