//! Opening an IMAP connection.
//!
//! The fetch provider and the IDLE watcher need the same connect → STARTTLS →
//! LOGIN preamble, so it is written once here. What differs is only the mailbox
//! mode they select, and that difference is the point: see [`connect`].

use std::time::Duration;

use crate::config::{ImapSettings, MailSettings, TlsMode};
use crate::error::MailError;
use crate::mail::tls;

use super::read::map_imap_error;
use super::ImapSession;

/// Commands are given this long, regardless of the configured value being
/// smaller: five seconds is the floor below which a slow link looks broken.
const MIN_COMMAND_TIMEOUT_SECS: u64 = 5;

/// Connect, authenticate and select the inbox.
///
/// `read_only` selects the mailbox with `EXAMINE` rather than `SELECT`, putting
/// the session into the server-enforced read-only state: a mutation attempted
/// on this connection is rejected by the server, not only by our own guard.
/// The IDLE watcher uses it because it never needs to set a flag; the fetch
/// path cannot, because under `work` it marks oversized messages as seen.
pub(crate) async fn connect(
    mail: &MailSettings,
    read_only: bool,
) -> Result<ImapSession, MailError> {
    let timeout = command_timeout(mail);
    if mail.username.trim().is_empty() {
        return Err(MailError::Auth("MAIL_IMAP_USER is empty".into()));
    }
    let mut client = async_imap::Client::new(tls::connect(&mail.imap).await?);

    if mail.imap.tls == TlsMode::StartTls {
        // The stream stays plaintext until this command succeeds, so no
        // credential may be sent before it does.
        tokio::time::timeout(timeout, client.run_command_and_check_ok("STARTTLS", None))
            .await
            .map_err(|_| MailError::Protocol("STARTTLS timed out".into()))?
            .map_err(map_imap_error)?;
        // No greeting follows STARTTLS, so the client is rebuilt around the
        // upgraded stream instead of continuing to read one.
        let plain = client.into_inner().into_inner()?;
        client = async_imap::Client::new(tls::upgrade_to_tls(&mail.imap, plain).await?);
    }

    let mut session = tokio::time::timeout(
        timeout,
        client.login(&mail.username, mail.password.expose()),
    )
    .await
    .map_err(|_| MailError::Auth("IMAP login timed out".into()))?
    .map_err(|(error, _client)| login_error(error))?;

    let inbox = folder(&mail.imap, mail.imap.inbox.as_str());
    if read_only {
        session.examine(inbox).await.map_err(map_imap_error)?;
    } else {
        session.select(inbox).await.map_err(map_imap_error)?;
    }
    Ok(session)
}

/// A rejected password arrives as a `NO` response, not as a transport failure.
///
/// Left to the generic mapper it would be filed under "protocol error" and the
/// retry loop would hammer a bad credential for ever; `Auth` is what tells the
/// caller to stop instead.
fn login_error(error: async_imap::error::Error) -> MailError {
    match error {
        async_imap::error::Error::No(message) => MailError::Auth(message),
        other => map_imap_error(other),
    }
}

/// Qualify a folder with the provider namespace prefix when configured.
pub(crate) fn folder(imap: &ImapSettings, name: &str) -> String {
    let name = name.trim();
    let prefix = imap.namespace_prefix.trim();
    if prefix.is_empty() || name.is_empty() {
        return name.to_string();
    }
    format!("{prefix}{name}")
}

pub(crate) fn command_timeout(mail: &MailSettings) -> Duration {
    Duration::from_secs(
        mail.imap
            .command_timeout_seconds
            .max(MIN_COMMAND_TIMEOUT_SECS),
    )
}
