//! Flag and lifecycle operations for the IMAP transport.

use super::read::{map_imap_error, mark_flagged};
use super::ImapMailProvider;
use crate::error::MailError;
use futures::StreamExt;

impl ImapMailProvider {
    /// `UID STORE +FLAGS (flag)`.
    pub(crate) async fn set_flag_impl(&self, uid: u32, flag: &str) -> Result<(), MailError> {
        let flag = flag.to_string();
        self.with_session(move |session| {
            Box::pin(async move { mark_flagged(session, uid, &flag).await })
        })
        .await
    }

    /// `UID STORE -FLAGS (flag)`.
    pub(crate) async fn clear_flag_impl(&self, uid: u32, flag: &str) -> Result<(), MailError> {
        let flag = flag.to_string();
        self.with_session(move |session| {
            Box::pin(async move {
                let query = format!("-FLAGS ({flag})");
                let mut stored = session
                    .uid_store(uid.to_string(), query)
                    .await
                    .map_err(map_imap_error)?;
                while let Some(result) = stored.next().await {
                    result.map_err(map_imap_error)?;
                }
                Ok(())
            })
        })
        .await
    }
}
