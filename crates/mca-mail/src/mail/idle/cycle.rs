//! One IDLE cycle: enter, wait for a change or a deadline, leave.
//!
//! RFC 2177 makes IDLE a command with a `DONE` terminator rather than a mode,
//! so every cycle ends by sending `DONE` and getting the plain session back.
//! Nothing here issues a mutation: the session was opened read-only, and the
//! only two commands sent are `IDLE` and `DONE`.

use std::time::Duration;

use async_imap::extensions::idle::IdleResponse;

use crate::error::MailError;
use crate::mail::imap::{read::map_imap_error, ImapSession};
use crate::shutdown::{stopping, Rx};

use super::{IdleError, IdleEvent};

/// Wait for the server to say something, or for `timeout` to pass.
///
/// Returns the session with IDLE already ended, so the caller can reuse it for
/// the next cycle or log it out. On error the session is gone: a protocol
/// failure leaves the stream in an unknown state, and reconnecting is cheaper
/// than diagnosing it.
pub(super) async fn run(
    session: ImapSession,
    timeout: Duration,
    shutdown: &mut Rx,
) -> Result<(IdleEvent, ImapSession), IdleError> {
    let mut handle = session.idle();
    if let Err(error) = handle.init().await {
        // The session is inside the handle; dropping it closes the socket, which
        // is exactly what a half-started `IDLE` needs.
        return Err(IdleError::Transient(map_imap_error(error)));
    }

    let (stopped, response) = await_response(&mut handle, timeout, shutdown).await?;
    let session = handle
        .done()
        .await
        .map_err(|error| IdleError::Transient(map_imap_error(error)))?;

    if stopped {
        return Ok((IdleEvent::Stopped, session));
    }
    Ok((event(response), session))
}

/// Block until the server responds, a deadline passes, or a stop arrives.
///
/// The `(stopped, response)` pair separates the two ways a wait can end on our
/// terms: the RFC's own timeout, which means "re-issue IDLE", and a shutdown,
/// which means "go home". Both still end IDLE with `DONE`.
async fn await_response(
    handle: &mut async_imap::extensions::idle::Handle<crate::mail::tls::ImapStreamKind>,
    timeout: Duration,
    shutdown: &mut Rx,
) -> Result<(bool, IdleResponse), IdleError> {
    let (idle, stop) = handle.wait_with_timeout(timeout);
    tokio::pin!(idle);

    let finished = tokio::select! {
        result = &mut idle => Some(result),
        _ = stopping(shutdown) => None,
    };
    let stopped = finished.is_none();

    let response = match finished {
        Some(result) => result.map_err(|error| IdleError::Transient(map_imap_error(error)))?,
        None => {
            // Dropping the stop source cancels the wait; awaiting it once more
            // lets the library unwind properly instead of abandoning a command
            // that is still outstanding on the wire.
            drop(stop);
            idle.await
                .map_err(|error| IdleError::Transient(map_imap_error(error)))?
        }
    };
    Ok((stopped, response))
}

/// `* 1 EXISTS` and friends all mean the same thing to us: look again.
fn event(response: IdleResponse) -> IdleEvent {
    match response {
        IdleResponse::NewData(_) => IdleEvent::NewMail,
        // `Timeout` is the 29-minute re-issue point; `ManualInterrupt` can only
        // come from a stop we have not seen yet, and is equally harmless.
        IdleResponse::Timeout | IdleResponse::ManualInterrupt => IdleEvent::Timeout,
    }
}

/// Does this server actually advertise IDLE?
///
/// RFC 3501: a client MUST NOT use `IDLE` unless the server says it supports
/// it, so the check is a hard gate rather than an optimisation.
pub(super) async fn supports_idle(session: &mut ImapSession) -> Result<bool, IdleError> {
    let capabilities = session
        .capabilities()
        .await
        .map_err(|error| IdleError::Transient(map_imap_error(error)))?;
    Ok(capabilities.has_str("IDLE"))
}

/// The refusal the worker logs once and then stops retrying on.
pub(super) fn unsupported() -> IdleError {
    IdleError::Unsupported(MailError::Protocol(
        "server does not advertise the IDLE capability".into(),
    ))
}
