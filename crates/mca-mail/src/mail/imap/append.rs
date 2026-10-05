//! APPEND and quarantine for the IMAP transport.

use super::read::map_imap_error;
use super::ImapMailProvider;
use crate::error::MailError;
use crate::mail::folders::FolderRole;

impl ImapMailProvider {
    /// `APPEND` a draft to the server drafts folder.
    ///
    /// Read-only refuses this before it is reached. A draft in the agent's own
    /// database is fine; a draft in the customer's mailbox is not.
    pub(crate) async fn append_draft_impl(
        &self,
        message: &crate::domain::OutboundMessage,
    ) -> Result<(), MailError> {
        let folder = self
            .resolve_folder(FolderRole::Drafts)
            .await?
            .ok_or_else(|| MailError::NoSuchFolder("drafts folder could not be resolved".into()))?;
        let raw = self.render(message)?;
        self.with_session(move |session| {
            Box::pin(async move {
                // `\Draft` is what makes the message visible in the client's
                // draft list rather than the inbox.
                session
                    .append(&folder, Some("(\\Draft)"), None, raw)
                    .await
                    .map_err(map_imap_error)
            })
        })
        .await
    }

    /// `APPEND` a message that already went out to the sent folder.
    ///
    /// This is what puts the reply in front of the customer in "Отправленные"
    /// as well as on the wire. Read-only refuses it first, and the caller
    /// treats a failure as a warning: the message is already gone, so
    /// reporting the failure as a delivery error would send it a second time.
    ///
    /// An explicitly configured folder wins, exactly as for spam — SPECIAL-USE
    /// `\Sent` is not advertised by every server, and plenty that do spell the
    /// folder differently.
    pub(crate) async fn append_sent_impl(
        &self,
        message: &crate::domain::OutboundMessage,
    ) -> Result<(), MailError> {
        let folder = match self.mail.sent_folder().map(str::to_owned) {
            Some(name) => self.folder(&name),
            None => self
                .resolve_folder(FolderRole::Sent)
                .await?
                .ok_or_else(|| {
                    MailError::NoSuchFolder("sent folder could not be resolved".into())
                })?,
        };
        let raw = self.render(message)?;
        self.with_session(move |session| {
            Box::pin(async move {
                // `\Seen`: a message we sent has already been read by us.
                session
                    .append(&folder, Some("(\\Seen)"), None, raw)
                    .await
                    .map_err(map_imap_error)
            })
        })
        .await
    }

    /// The bytes an `APPEND` would store: the same renderer SMTP uses, with
    /// the sender identity SMTP used, so the archived copy matches the send.
    fn render(&self, message: &crate::domain::OutboundMessage) -> Result<Vec<u8>, MailError> {
        crate::mail::render::rfc5322(message, self.smtp.from_address(), self.smtp.from_name())
    }
}
