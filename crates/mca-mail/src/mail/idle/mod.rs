//! Event-driven mailbox notification over IMAP IDLE (RFC 2177).
//!
//! Polling waits for the next tick; IDLE lets the server say "there is new
//! mail" the moment it arrives. It is an *additional* signal, never a
//! replacement: the fallback poller keeps running on a longer period, because
//! an IDLE connection that silently died would otherwise stop mail forever
//! rather than merely slowing it down.
//!
//! The split of responsibility:
//!
//! * [`IdleSource`] — one wait on one connection. Knows the protocol, not the
//!   database, and never touches a flag: the watcher's session is opened
//!   `EXAMINE`d, so the server itself would refuse a mutation.
//! * the worker in `application::workers::idle` — owns backoff, reconnect and
//!   the shutdown handshake.
//! * `sync_loop` — the single place that actually reads the mailbox, shared by
//!   IDLE, the fallback timer and the plain interval.

mod backoff;
mod cycle;
mod imap;

#[cfg(test)]
#[path = "idle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "scripted.rs"]
pub(crate) mod scripted;

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::{MailProviderKind, MailSettings};
use crate::error::MailError;
use crate::shutdown::Rx;

pub use backoff::Backoff;
pub use imap::ImapIdleSource;

/// What the server did while we waited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleEvent {
    /// Something changed in the mailbox: read it now.
    NewMail,
    /// The RFC's own re-issue point, or an unchanged keep-alive: just go again.
    Timeout,
    /// The process is going down: leave and do not come back.
    Stopped,
}

/// Why a wait produced no event.
#[derive(Debug)]
pub enum IdleError {
    /// Worth retrying after a backoff — a dropped socket, a timed-out command.
    Transient(MailError),
    /// Not worth retrying: IDLE is unsupported, or the credentials are wrong.
    /// Retrying would turn a misconfiguration into a log flood.
    Unsupported(MailError),
}

/// One waiter against one mailbox, hiding whether it is real or scripted.
#[async_trait]
pub trait IdleSource: Send + Sync {
    /// Block until the server reports a change or `shutdown` is requested.
    async fn wait(&self, shutdown: &mut Rx) -> Result<IdleEvent, IdleError>;

    /// Log out if a session is open. Safe to call twice.
    async fn close(&self);
}

/// The event source this configuration should use, or `None` if none should.
///
/// The two conditions are independent on purpose: `MAIL_IDLE` is the operator's
/// switch, while `MAIL_PROVIDER=imap` is what makes it meaningful. The mock
/// provider has no socket to idle on.
pub fn build(settings: &MailSettings) -> Option<Arc<dyn IdleSource>> {
    if settings.provider != MailProviderKind::Imap || !settings.imap.use_idle {
        return None;
    }
    Some(Arc::new(ImapIdleSource::new(settings)))
}
