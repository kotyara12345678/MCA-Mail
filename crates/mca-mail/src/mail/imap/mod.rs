//! The IMAP transport: one connection, one mailbox, strict read/write split.

use std::pin::Pin;

use crate::error::MailError;

mod append;
mod flags;
mod mutations;
mod provider;
pub(crate) mod read;
mod session;
#[cfg(test)]
mod timeout_tests;
mod writer;

/// After `LOGIN` the client becomes a `Session`; the two differ only in that the
/// session also exposes the unsolicited-response channel.
pub(crate) type ImapSession = async_imap::Session<crate::mail::tls::ImapStreamKind>;

type SessionOp<'a, T> =
    Pin<Box<dyn std::future::Future<Output = Result<T, MailError>> + Send + 'a>>;

pub struct ImapMailProvider {
    pub(crate) mail: crate::config::MailSettings,
    pub(crate) smtp: super::smtp::SmtpTransport,
    pub(crate) guard: crate::mail::guard::MailGuard,
    open: tokio::sync::Mutex<Option<Open>>,
}

struct Open {
    session: Box<ImapSession>,
    last_success_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ImapMailProvider {
    /// `smtp` already holds the same credentials, so the provider does not keep
    /// a second copy of the password.
    ///
    /// The guard is built from the configured `MailMode`; every mutating method
    /// in [`MailboxWriter`](crate::mail::MailboxWriter) consults it before
    /// touching the session.
    pub fn new(mail: &crate::config::MailSettings, smtp: super::smtp::SmtpTransport) -> Self {
        Self {
            mail: mail.clone(),
            smtp,
            guard: crate::mail::guard::MailGuard::new(mail.mode),
            open: tokio::sync::Mutex::new(None),
        }
    }

    pub fn guard(&self) -> crate::mail::guard::MailGuard {
        self.guard
    }

    pub(crate) fn imap(&self) -> &crate::config::ImapSettings {
        &self.mail.imap
    }

    pub fn smtp(&self) -> &super::smtp::SmtpTransport {
        &self.smtp
    }

    /// Qualify a folder with the provider namespace prefix when configured.
    pub(crate) fn folder(&self, name: &str) -> String {
        session::folder(self.imap(), name)
    }

    pub(crate) async fn list_folders(
        &self,
    ) -> Result<Vec<crate::mail::folders::FolderInfo>, MailError> {
        self.with_session(|session| {
            Box::pin(async move {
                use futures::StreamExt;
                let mut listed = session
                    .list(None, Some("*"))
                    .await
                    .map_err(read::map_imap_error)?;
                let mut folders = Vec::new();
                while let Some(item) = listed.next().await {
                    let item = item.map_err(read::map_imap_error)?;
                    folders.push(crate::mail::folders::FolderInfo::from_list(
                        item.name(),
                        item.attributes(),
                    ));
                }
                Ok(folders)
            })
        })
        .await
    }

    pub(crate) async fn resolve_folder(
        &self,
        role: crate::mail::folders::FolderRole,
    ) -> Result<Option<String>, MailError> {
        let folders = self.list_folders().await?;
        Ok(crate::mail::folders::resolve(role, &folders).map(str::to_string))
    }

    /// Run `op` against a live session, opening or re-opening it as needed.
    ///
    /// The lock is held for the whole operation: IMAP forbids interleaving
    /// commands on one connection, so two pollers sharing a session would
    /// interleave `SELECT`/`FETCH` and mis-attribute the results.
    pub(crate) async fn with_session<T, F>(&self, op: F) -> Result<T, MailError>
    where
        T: Send,
        F: for<'a> FnOnce(&'a mut ImapSession) -> SessionOp<'a, T>,
    {
        let mut guard = self.open.lock().await;
        if guard.is_none() {
            *guard = Some(Open {
                session: Box::new(self.open_session().await?),
                last_success_at: None,
            });
        }
        // A command the server never answers would otherwise hold this lock for
        // ever: every later IMAP operation queues behind it, so the mailbox
        // stops moving while the health check keeps reporting healthy. The
        // deadline is the same one login and STARTTLS already use.
        let timeout = session::command_timeout(&self.mail);
        let open = guard.as_mut().expect("session just established");
        let outcome = tokio::time::timeout(timeout, op(&mut open.session)).await;
        match outcome {
            Ok(Ok(value)) => {
                open.last_success_at = Some(chrono::Utc::now());
                Ok(value)
            }
            Ok(Err(error)) => {
                // Any protocol error leaves the stream in an unknown state.
                // Dropping it is cheaper than diagnosing a desynchronised session.
                if is_fatal(&error) {
                    *guard = None;
                }
                Err(error)
            }
            Err(_elapsed) => {
                // The reply is not coming, so where the stream stands in the
                // protocol is unknown. Dropping the session lets the next call
                // reconnect instead of waiting for an answer that never arrives.
                *guard = None;
                Err(MailError::Unavailable(format!(
                    "imap command timed out after {}s",
                    timeout.as_secs()
                )))
            }
        }
    }

    /// The fetch path runs read-write: under `work` it has to set `\Seen`.
    async fn open_session(&self) -> Result<ImapSession, MailError> {
        session::connect(&self.mail, fetch_session_read_only(self.guard.mode())).await
    }

    /// Close the session politely on shutdown.
    pub(crate) async fn close_session(&self) -> Result<(), MailError> {
        let mut guard = self.open.lock().await;
        let Some(mut open) = guard.take() else {
            return Ok(());
        };
        // The session is already out of the pool, so a `LOGOUT` the server
        // ignores must not be able to hold shutdown open for ever.
        tokio::time::timeout(session::command_timeout(&self.mail), open.session.logout())
            .await
            .map_err(|_| MailError::Unavailable("imap logout timed out".into()))?
            .map_err(read::map_imap_error)
    }
}

/// Open a throwaway connection for the IDLE watcher.
///
/// It reads the same settings as the fetch session but selects the mailbox
/// read-only, so this connection can never be used to change a flag — see
/// [`session::connect`].
pub(crate) async fn open_read_only_session(
    mail: &crate::config::MailSettings,
) -> Result<ImapSession, MailError> {
    session::connect(mail, true).await
}

fn is_fatal(error: &MailError) -> bool {
    matches!(
        error,
        MailError::Unavailable(_) | MailError::Auth(_) | MailError::Connect(_)
    )
}

fn fetch_session_read_only(mode: crate::config::MailMode) -> bool {
    !mode.allows_mailbox_write()
}

#[cfg(test)]
mod tests {
    use crate::config::MailMode;

    use super::fetch_session_read_only;

    #[test]
    fn read_only_mode_uses_a_server_read_only_session() {
        assert!(fetch_session_read_only(MailMode::ReadOnly));
        assert!(!fetch_session_read_only(MailMode::Work));
    }
}
