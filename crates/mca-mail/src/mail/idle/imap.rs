//! The real IDLE source: one read-only connection, reused across cycles.

use std::time::Duration;

use async_trait::async_trait;

use crate::config::MailSettings;
use crate::error::MailError;
use crate::mail::imap::{self, ImapSession};
use crate::shutdown::Rx;

use super::{cycle, IdleError, IdleEvent, IdleSource};

/// A dedicated session, kept between waits so a burst of notifications does not
/// reconnect after each one.
///
/// It is deliberately *not* the provider's session: sharing it would park the
/// fetch path behind a `&mut` that only releases at `DONE`, which is exactly
/// when there is new mail to fetch.
pub struct ImapIdleSource {
    mail: MailSettings,
    wait: Duration,
    session: tokio::sync::Mutex<Option<ImapSession>>,
}

impl ImapIdleSource {
    pub fn new(mail: &MailSettings) -> Self {
        Self {
            mail: mail.clone(),
            wait: mail.imap.idle.wait(),
            session: tokio::sync::Mutex::new(None),
        }
    }

    /// Open a fresh `EXAMINE`d connection, or say why we must stop trying.
    ///
    /// `Unsupported` is the terminal case: without it a server that never
    /// advertises `IDLE`, or a rejected password, would be re-dialled every
    /// backoff interval until someone reads the log.
    async fn open(&self) -> Result<ImapSession, IdleError> {
        let mut session = imap::open_read_only_session(&self.mail)
            .await
            .map_err(classify)?;
        if !cycle::supports_idle(&mut session).await? {
            let _ = session.logout().await;
            return Err(cycle::unsupported());
        }
        Ok(session)
    }
}

fn classify(error: MailError) -> IdleError {
    match error {
        MailError::Auth(_) => IdleError::Unsupported(error),
        other => IdleError::Transient(other),
    }
}

#[async_trait]
impl IdleSource for ImapIdleSource {
    async fn wait(&self, shutdown: &mut Rx) -> Result<IdleEvent, IdleError> {
        let mut slot = self.session.lock().await;
        let session = match slot.take() {
            Some(session) => session,
            None => self.open().await?,
        };
        let (event, session) = cycle::run(session, self.wait, shutdown).await?;
        *slot = Some(session);
        Ok(event)
    }

    async fn close(&self) {
        let mut slot = self.session.lock().await;
        if let Some(mut session) = slot.take() {
            if let Err(error) = session.logout().await {
                tracing::debug!(%error, "IDLE logout failed");
            }
        }
    }
}
