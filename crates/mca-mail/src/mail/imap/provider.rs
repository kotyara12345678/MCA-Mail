//! The read side of the transport: fetch, health, capability name.

use super::read::map_imap_error;
use super::ImapMailProvider;
use crate::domain::ChannelHealth;
use crate::error::MailError;
use crate::mail::MailProvider;
use async_trait::async_trait;

/// Reads only. Sending lives on `MailboxWriter`, deliberately out of reach of
/// the `&dyn MailProvider` that the read-only pipeline holds.
#[async_trait]
impl MailProvider for ImapMailProvider {
    fn name(&self) -> &'static str {
        "imap"
    }

    async fn init(&self) -> Result<(), MailError> {
        let capabilities = self
            .with_session(|session| {
                Box::pin(async move { session.capabilities().await.map_err(map_imap_error) })
            })
            .await?;
        tracing::info!(
            move = capabilities.has_str("MOVE"),
            idle = capabilities.has_str("IDLE"),
            special_use = capabilities.has_str("SPECIAL-USE"),
            "IMAP capabilities"
        );
        let folders = self.list_folders().await?;
        for folder in folders {
            let special_use = if folder.special_use.is_empty() {
                "none".to_string()
            } else {
                folder.special_use.join(",")
            };
            tracing::info!(
                name = %folder.name,
                display_name = "unknown",
                exists = true,
                role = folder.role.as_str(),
                special_use = %special_use,
                is_inbox = folder.role == crate::mail::folders::FolderRole::Inbox,
                is_drafts = folder.role == crate::mail::folders::FolderRole::Drafts,
                is_sent = folder.role == crate::mail::folders::FolderRole::Sent,
                is_spam = folder.role == crate::mail::folders::FolderRole::Spam,
                is_trash = folder.role == crate::mail::folders::FolderRole::Trash,
                is_archive = folder.role == crate::mail::folders::FolderRole::Archive,
                is_user_folder = "unknown",
                selectable = folder.selectable,
                readable = if folder.selectable { "unknown" } else { "false" },
                movable = "unknown",
                "FOLDER"
            );
        }
        Ok(())
    }

    /// Flag-driven view of "unread", kept for the channel-neutral fetch and
    /// tooling. The poll cycle does not use it: newness is decided by the
    /// persisted UID watermark (see [`MailProvider::fetch_after`]), because a
    /// read-only deployment cannot set `\Seen` and a flag scan re-reads the
    /// whole history after every restart.
    async fn fetch_new(&self) -> Result<Vec<crate::domain::InboundMessage>, MailError> {
        let limit = self.mail.fetch_batch_size.max(1);
        let (raw, _) = self.fetch_uid_range("UNSEEN", 0, limit).await?;
        self.parse_batch(raw).await
    }

    /// UIDVALIDITY and UIDNEXT straight from `STATUS`.
    ///
    /// Read fresh each cycle rather than cached from `SELECT`: a session may
    /// silently reconnect between polls, and the initial boundary must be the
    /// `UIDNEXT` of the moment the cursor is written, not of process start.
    async fn uid_state(&self) -> Result<crate::mail::UidState, MailError> {
        let folder = self.folder(self.mail.imap.inbox.as_str());
        self.with_session(move |session| {
            Box::pin(async move {
                let selected = session
                    .status(&folder, "(UIDVALIDITY UIDNEXT)")
                    .await
                    .map_err(map_imap_error)?;
                let uid_validity = selected.uid_validity.ok_or_else(|| {
                    MailError::Protocol("server did not report UIDVALIDITY".into())
                })?;
                let uid_next = match selected.uid_next {
                    Some(next) => next,
                    None => uid_next_from_search(session).await?,
                };
                Ok(crate::mail::UidState {
                    uid_validity,
                    uid_next,
                })
            })
        })
        .await
    }

    /// Messages newer than the watermark: `UID (after+1):*`, sorted and cut
    /// to one batch inside `fetch_uid_range`.
    async fn fetch_after(&self, after_uid: u32) -> Result<crate::mail::FetchBatch, MailError> {
        let limit = self.mail.fetch_batch_size.max(1);
        // `saturating_add` keeps a boundary at `u32::MAX` from wrapping the
        // range around to UID 0 — the whole mailbox — which would be a backfill.
        let query = format!("UID {}:*", after_uid.saturating_add(1));
        let (raw, highest_uid) = self.fetch_uid_range(&query, after_uid, limit).await?;
        let messages = self.parse_batch(raw).await?;
        Ok(crate::mail::FetchBatch {
            messages,
            highest_uid,
        })
    }

    async fn health(&self) -> ChannelHealth {
        let guard = self.open.lock().await;
        let detail = format!("imap {}", self.imap().host);
        match guard.as_ref() {
            Some(open) => ChannelHealth {
                connected: true,
                detail: Some(detail),
                last_success_at: open.last_success_at,
            },
            None => ChannelHealth {
                connected: false,
                detail: Some(format!("{detail}: not connected")),
                last_success_at: None,
            },
        }
    }
}

/// UIDNEXT absent (non-compliant server): derive it from the highest UID that
/// exists. An empty mailbox reports 1, keeping the boundary at 0 — safe,
/// because there is nothing on the server that a backfill could replay.
async fn uid_next_from_search(session: &mut super::ImapSession) -> Result<u32, MailError> {
    let uids = session.uid_search("ALL").await.map_err(map_imap_error)?;
    Ok(uids
        .iter()
        .copied()
        .max()
        .map_or(1, |max| max.saturating_add(1)))
}
