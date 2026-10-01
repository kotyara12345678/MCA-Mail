//! Live IMAP transport.
//!
//! One authenticated session is held behind a mutex and reused across polls.
//! IMAP connections are stateful and expensive to rebuild (TLS handshake,
//! login, `SELECT`), so a connection per poll would be slow and would get the
//! account rate-limited.

use async_imap::types::Fetch;
use async_imap::{Client, Session};
use futures::StreamExt;
use tokio::sync::Mutex;

use crate::config::{ImapSettings, MailSettings, TlsMode};
use crate::domain::{ChannelHealth, InboundMessage, OutboundMessage};
use crate::error::MailError;

use super::smtp::SmtpTransport;
use super::tls::{self, ImapStreamKind};
use super::MailProvider;

/// After `LOGIN` the client becomes a `Session`; the two differ only in that
/// the session also exposes the unsolicited-response channel.
type ImapSession = Session<ImapStreamKind>;

/// A session operation as a boxed future, so one closure type works for every
/// borrow of the session rather than forcing a single concrete lifetime.
type SessionOp<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, MailError>> + Send + 'a>>;

struct Open {
    session: Box<ImapSession>,
    last_success_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct ImapMailProvider {
    mail: MailSettings,
    smtp: SmtpTransport,
    open: Mutex<Option<Open>>,
}

impl ImapMailProvider {
    /// `smtp` already holds the same credentials, so the provider does not keep
    /// a second copy of the password.
    pub fn new(mail: &MailSettings, smtp: SmtpTransport) -> Self {
        Self {
            mail: mail.clone(),
            smtp,
            open: Mutex::new(None),
        }
    }

    fn imap(&self) -> &ImapSettings {
        &self.mail.imap
    }

    fn command_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.imap().command_timeout_seconds.max(5))
    }

    /// Qualify a folder with the provider namespace prefix when configured.
    fn folder(&self, name: &str) -> String {
        let name = name.trim();
        let prefix = self.imap().namespace_prefix.trim();
        if prefix.is_empty() || name.is_empty() {
            return name.to_string();
        }
        format!("{prefix}{name}")
    }

    /// Run `op` against a live session, opening or re-opening it as needed.
    ///
    /// The lock is held for the whole operation: IMAP forbids interleaving
    /// commands on one connection, so two pollers sharing a session would
    /// interleave `SELECT`/`FETCH` and mis-attribute the results.
    async fn with_session<T, F>(&self, op: F) -> Result<T, MailError>
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
        let open = guard.as_mut().expect("session just established");
        match op(&mut open.session).await {
            Ok(value) => {
                open.last_success_at = Some(chrono::Utc::now());
                Ok(value)
            }
            Err(error) => {
                // Any protocol error leaves the stream in an unknown state.
                // Dropping it is cheaper than diagnosing a desynchronised session.
                if is_fatal(&error) {
                    *guard = None;
                }
                Err(error)
            }
        }
    }

    async fn open_session(&self) -> Result<ImapSession, MailError> {
        let settings = self.imap();
        let timeout = self.command_timeout();
        if self.mail.username.trim().is_empty() {
            return Err(MailError::Auth("MAIL_IMAP_USER is empty".into()));
        }
        let mut client = Client::new(tls::connect(settings).await?);

        if settings.tls == TlsMode::StartTls {
            // The stream stays plaintext until this command succeeds, so no
            // credential may be sent before it does.
            tokio::time::timeout(timeout, client.run_command_and_check_ok("STARTTLS", None))
                .await
                .map_err(|_| MailError::Protocol("STARTTLS timed out".into()))?
                .map_err(map_imap_error)?;
            // No greeting follows STARTTLS, so the client is rebuilt around the
            // upgraded stream instead of continuing to read one.
            let plain = client.into_inner().into_inner()?;
            client = Client::new(tls::upgrade_to_tls(settings, plain).await?);
        }

        let session = tokio::time::timeout(
            timeout,
            client.login(&self.mail.username, self.mail.password.expose()),
        )
        .await
        .map_err(|_| MailError::Auth("IMAP login timed out".into()))?
        .map_err(|(error, _client)| map_imap_error(error))?;

        let mut session = session;
        session
            .select(self.folder(&settings.inbox))
            .await
            .map_err(map_imap_error)?;
        Ok(session)
    }

    /// Fetch unseen messages without marking them seen.
    ///
    /// `BODY.PEEK[]` leaves `\Seen` untouched, so a crash between fetch and
    /// database commit cannot silently lose a message. Flagging happens in
    /// [`Self::mark_processed`] once the row is committed.
    async fn fetch_unseen(&self, limit: usize) -> Result<Vec<(u32, Vec<u8>)>, MailError> {
        self.with_session(move |session| {
            Box::pin(async move {
                let uids = session.uid_search("UNSEEN").await.map_err(map_imap_error)?;
                let selected: Vec<String> = uids.iter().take(limit).map(u32::to_string).collect();
                if selected.is_empty() {
                    return Ok(Vec::new());
                }
                // The fetch stream borrows the session for its whole lifetime, so
                // bodies are collected before any further command is issued.
                let (accepted, oversized) = {
                    let mut messages = session
                        .uid_fetch(selected.join(","), "(UID RFC822.SIZE BODY.PEEK[])")
                        .await
                        .map_err(map_imap_error)?;
                    let mut accepted = Vec::new();
                    let mut oversized = Vec::new();
                    while let Some(fetched) = messages.next().await {
                        let fetched: Fetch = fetched.map_err(map_imap_error)?;
                        let Some(uid) = fetched.uid else { continue };
                        let Some(raw) = fetched.text().or_else(|| fetched.body()) else {
                            tracing::warn!(uid, "server returned a fetch without a body");
                            continue;
                        };
                        if raw.len() > super::parse::MAX_MESSAGE_BYTES {
                            oversized.push(uid);
                            continue;
                        }
                        accepted.push((uid, raw.to_vec()));
                    }
                    (accepted, oversized)
                };

                // An oversized message must not be returned on every poll, so it
                // is flagged here and recorded by the caller's log line.
                for uid in oversized {
                    tracing::warn!(uid, "inbound message exceeds size guard, skipped");
                    mark_flagged(session, uid, "\\Seen").await.ok();
                }
                Ok(accepted)
            })
        })
        .await
    }

    /// Record that a message reached the database.
    pub async fn mark_processed(&self, uid: u32) -> Result<(), MailError> {
        self.with_session(move |session| Box::pin(mark_flagged(session, uid, "\\Seen")))
            .await
    }

    /// Move a message into the quarantine folder instead of processing it.
    ///
    /// With no quarantine folder configured the message is only flagged: a
    /// server without that folder answers `NO` to `UID MOVE`, and retrying it
    /// forever would block the poll cycle.
    pub async fn quarantine(&self, uid: u32, reason: &str) -> Result<(), MailError> {
        let folder = self.folder(self.imap().quarantine_folder.as_str());
        let reason = reason.to_string();
        self.with_session(move |session| {
            Box::pin(async move {
                if folder.is_empty() {
                    tracing::info!(uid, %reason, "quarantine folder unset, message flagged only");
                    return mark_flagged(session, uid, "\\Seen").await;
                }
                // The junk label is best-effort: not every server allows setting
                // it, and failing here would skip the move that does matter.
                mark_flagged(session, uid, "$Junk").await.ok();
                session
                    .uid_mv(uid.to_string(), &folder)
                    .await
                    .map_err(map_imap_error)
            })
        })
        .await
    }

    /// Append a draft to the server drafts folder.
    ///
    /// `APPEND` does not return a UID in this protocol version, so the caller
    /// records the draft locally and treats the append as fire-and-forget.
    pub async fn save_draft(&self, message: &OutboundMessage) -> Result<(), MailError> {
        let folder = self.folder(self.imap().drafts_folder.as_str());
        let raw = super::render::rfc5322(message, self.smtp.from_address())?;
        self.with_session(move |session| {
            Box::pin(async move {
                if folder.is_empty() {
                    return Err(MailError::NoSuchFolder(
                        "MAIL_IMAP_DRAFTS_FOLDER is empty".into(),
                    ));
                }
                // `\Draft` is what makes the message visible in the client's
                // draft list rather than the inbox.
                session
                    .append(&folder, Some("\\Draft"), None, raw)
                    .await
                    .map_err(map_imap_error)
            })
        })
        .await
    }

    /// Close the session politely on shutdown, so the server releases the
    /// mailbox for the next process instead of waiting for a TCP timeout.
    pub async fn close(&self) -> Result<(), MailError> {
        let mut guard = self.open.lock().await;
        let Some(mut open) = guard.take() else {
            return Ok(());
        };
        open.session.logout().await.map_err(map_imap_error)
    }

    pub fn smtp(&self) -> &SmtpTransport {
        &self.smtp
    }
}

#[async_trait::async_trait]
impl MailProvider for ImapMailProvider {
    fn name(&self) -> &'static str {
        "imap"
    }

    async fn fetch_new(&self) -> Result<Vec<InboundMessage>, MailError> {
        let limit = self.mail.fetch_batch_size.max(1);
        let raw_messages = self.fetch_unseen(limit).await?;
        let mut out = Vec::with_capacity(raw_messages.len());
        for (uid, raw) in raw_messages {
            match super::parse::parse(&raw, uid.to_string()) {
                Ok(message) => out.push(message),
                Err(error) => {
                    // Unparseable mail must not block the batch: flag it, record
                    // why, and continue with the rest.
                    tracing::warn!(uid, %error, "skipping unparseable message");
                    let _ = self.quarantine(uid, "unparseable").await;
                }
            }
        }
        Ok(out)
    }

    async fn send(&self, message: &OutboundMessage) -> Result<String, MailError> {
        self.smtp.send_raw(message).await
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

async fn mark_flagged(session: &mut ImapSession, uid: u32, flag: &str) -> Result<(), MailError> {
    let query = format!("+FLAGS ({flag})");
    let mut stored = session
        .uid_store(uid.to_string(), query)
        .await
        .map_err(map_imap_error)?;
    while let Some(result) = stored.next().await {
        result.map_err(map_imap_error)?;
    }
    Ok(())
}

pub fn map_imap_error(error: async_imap::error::Error) -> MailError {
    match error {
        async_imap::error::Error::ConnectionLost => {
            MailError::Unavailable("imap connection lost".into())
        }
        // `NO` covers both a rejected command and a bad login; the message text
        // is preserved so the operator can tell them apart in the log.
        other => MailError::Protocol(other.to_string()),
    }
}

fn is_fatal(error: &MailError) -> bool {
    matches!(
        error,
        MailError::Unavailable(_) | MailError::Auth(_) | MailError::Connect(_)
    )
}
