use async_trait::async_trait;

use super::ImapMailProvider;
use crate::domain::OutboundMessage;
use crate::error::MailError;
use crate::mail::guard::MailGuard;
use crate::mail::ops::MailboxOp;
use crate::mail::writer::MailboxWriter;

/// The only place the guard is consulted for this transport.
///
/// Every method checks first and issues no command on refusal, so a read-only
/// deployment cannot reach the server even once. Keeping the checks in one impl
/// block is what makes that auditable: a new mutation cannot be added elsewhere
/// without either landing here or being unreachable through the trait.
#[async_trait]
impl MailboxWriter for ImapMailProvider {
    fn guard(&self) -> MailGuard {
        self.guard
    }

    async fn send(&self, message: &OutboundMessage) -> Result<String, MailError> {
        self.guard.check(MailboxOp::Send, None)?;
        self.smtp.send_raw(message).await
    }

    async fn append_draft(&self, message: &OutboundMessage) -> Result<(), MailError> {
        self.guard.check(MailboxOp::AppendDraft, None)?;
        self.append_draft_impl(message).await
    }

    async fn append_sent(&self, message: &OutboundMessage) -> Result<(), MailError> {
        self.guard.check(MailboxOp::AppendSent, None)?;
        self.append_sent_impl(message).await
    }

    async fn move_to_role(
        &self,
        uid: u32,
        role: crate::mail::folders::FolderRole,
    ) -> Result<(), MailError> {
        self.guard.check(MailboxOp::Move, Some(&uid.to_string()))?;
        self.move_to_role_impl(uid, role).await
    }

    async fn copy_to_role(
        &self,
        uid: u32,
        role: crate::mail::folders::FolderRole,
    ) -> Result<(), MailError> {
        self.guard.check(MailboxOp::Copy, Some(&uid.to_string()))?;
        self.copy_to_role_impl(uid, role).await
    }

    async fn delete_message(&self, uid: u32) -> Result<(), MailError> {
        self.guard
            .check(MailboxOp::Delete, Some(&uid.to_string()))?;
        Err(MailError::OperationNotAllowed {
            op: MailboxOp::Delete.as_str().to_string(),
            mode: self.guard.mode().as_str().to_string(),
        })
    }

    async fn set_flag(&self, uid: u32, flag: &str) -> Result<(), MailError> {
        self.guard
            .check_flag(MailboxOp::SetFlag, Some(&uid.to_string()), flag)?;
        self.set_flag_impl(uid, flag).await
    }

    async fn clear_flag(&self, uid: u32, flag: &str) -> Result<(), MailError> {
        self.guard
            .check_flag(MailboxOp::ClearFlag, Some(&uid.to_string()), flag)?;
        self.clear_flag_impl(uid, flag).await
    }

    async fn quarantine(&self, uid: u32, reason: &str) -> Result<(), MailError> {
        self.guard.check(MailboxOp::Move, Some(&uid.to_string()))?;
        // An explicitly configured folder wins: the Spam role is resolved from
        // SPECIAL-USE flags only, and plenty of servers never advertise them.
        if let Some(name) = self.mail.quarantine_folder().map(str::to_owned) {
            let folder = self.folder(&name);
            tracing::debug!(uid, %reason, folder = %folder, "moving to configured quarantine folder");
            return self.move_impl(uid, &folder).await;
        }
        tracing::debug!(uid, %reason, "resolving spam folder role");
        self.move_to_role_impl(uid, crate::mail::folders::FolderRole::Spam)
            .await
    }

    /// Record that a message's row was committed.
    ///
    /// Refused under read-only, so the message is re-read next poll. That is the
    /// right trade-off: the row is already committed, and the pipeline's
    /// idempotency key absorbs the repeat.
    async fn mark_processed(&self, uid: u32) -> Result<(), MailError> {
        self.guard
            .check(MailboxOp::SetFlag, Some(&uid.to_string()))?;
        self.set_flag_impl(uid, "\\Seen").await
    }

    async fn close(&self) -> Result<(), MailError> {
        self.close_session().await
    }
}
