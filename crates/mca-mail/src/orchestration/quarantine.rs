//! Moving a message the spam stage convicted out of the inbox.

use tracing::{info, warn};

use super::Orchestrator;
use crate::domain::StoredEmail;
use crate::mail::MailboxWriter;
use crate::observability::Correlation;

impl Orchestrator {
    /// Move a convicted message into the quarantine folder.
    ///
    /// Best effort by design. The row already says `quarantined`, so a refused
    /// or failed move is logged and the message stays in the inbox, where an
    /// operator can still find it. Failing the run instead would throw the
    /// verdict away along with the transport.
    pub(super) async fn quarantine_spam(&self, corr: &Correlation, email: &StoredEmail) {
        if !self.config.security.email_mode.allows_mailbox_mutation() {
            warn!(
                email_id = %email.id,
                processing_id = %corr.processing_id,
                "mailbox mutation disabled; message left in the inbox"
            );
            return;
        }
        let Some(mailbox) = self.mailbox.as_ref() else {
            warn!(
                email_id = %email.id,
                processing_id = %corr.processing_id,
                "no mailbox handle; message left in the inbox"
            );
            return;
        };
        let uid = match email.provider_uid.as_deref().map(str::parse::<u32>) {
            Some(Ok(uid)) => uid,
            _ => {
                warn!(
                    email_id = %email.id,
                    processing_id = %corr.processing_id,
                    "message carries no server UID; cannot move it"
                );
                return;
            }
        };
        match MailboxWriter::quarantine(&**mailbox, uid, "spam stage").await {
            Ok(()) => info!(
                email_id = %email.id,
                processing_id = %corr.processing_id,
                uid,
                "message moved to the quarantine folder"
            ),
            Err(error) => warn!(
                email_id = %email.id,
                processing_id = %corr.processing_id,
                uid,
                %error,
                "quarantine move failed; message left in the inbox"
            ),
        }
    }
}
