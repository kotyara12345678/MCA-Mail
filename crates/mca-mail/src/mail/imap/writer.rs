//! Folder and copy operations for the IMAP transport.
//!
//! These are inherent methods rather than trait methods so the file size limit
//! can be respected; the single `MailboxWriter` impl in `super` delegates here
//! after checking the guard.

use futures::StreamExt;

use super::read::map_imap_error;
use super::ImapMailProvider;
use crate::error::MailError;

impl ImapMailProvider {
    pub(crate) async fn move_to_role_impl(
        &self,
        uid: u32,
        role: crate::mail::folders::FolderRole,
    ) -> Result<(), MailError> {
        let Some(folder) = self.resolve_folder(role).await? else {
            tracing::warn!(role = role.as_str(), "folder role could not be resolved");
            return Err(MailError::NoSuchFolder(format!(
                "{} folder could not be resolved",
                role.as_str()
            )));
        };
        self.move_impl(uid, &folder).await
    }

    pub(crate) async fn copy_to_role_impl(
        &self,
        uid: u32,
        role: crate::mail::folders::FolderRole,
    ) -> Result<(), MailError> {
        let Some(folder) = self.resolve_folder(role).await? else {
            tracing::warn!(role = role.as_str(), "folder role could not be resolved");
            return Err(MailError::NoSuchFolder(format!(
                "{} folder could not be resolved",
                role.as_str()
            )));
        };
        self.copy_impl(uid, &folder).await
    }

    /// `UID MOVE` a message to another folder.
    pub(crate) async fn move_impl(&self, uid: u32, folder: &str) -> Result<(), MailError> {
        let folder = folder.to_string();
        self.with_session(move |session| {
            Box::pin(async move {
                session
                    .uid_mv(uid.to_string(), &folder)
                    .await
                    .map_err(map_imap_error)
            })
        })
        .await
    }

    /// `UID COPY` a message to another folder. The copy is the non-destructive
    /// counterpart to a move, so a bad classification can be undone.
    ///
    /// `uid_copy` in this protocol version returns a bare acknowledgement rather
    /// than the destination UID stream `uid_copy_stream` yields, so there is
    /// nothing to drain here.
    async fn copy_impl(&self, uid: u32, folder: &str) -> Result<(), MailError> {
        let folder = folder.to_string();
        self.with_session(move |session| {
            Box::pin(async move {
                session
                    .uid_copy(uid.to_string(), &folder)
                    .await
                    .map_err(map_imap_error)
            })
        })
        .await
    }
}
