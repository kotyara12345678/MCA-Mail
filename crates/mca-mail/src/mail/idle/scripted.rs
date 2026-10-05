//! A scripted [`IdleSource`], so worker tests need neither a socket nor luck.
//!
//! A real server would make every assertion about timing depend on the network;
//! a queued sequence of outcomes makes the same assertion about ordering.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;

use super::{IdleError, IdleEvent, IdleSource};
use crate::shutdown::Rx;

/// Answers each `wait` with the next queued outcome.
///
/// An empty queue means the test scripted fewer rounds than the loop actually
/// needs, so it panics with the call count instead of letting the loop sleep
/// through a wrong expectation.
pub struct ScriptedIdle {
    events: Mutex<VecDeque<Result<IdleEvent, IdleError>>>,
    closes: AtomicUsize,
}

impl ScriptedIdle {
    pub fn new(events: Vec<Result<IdleEvent, IdleError>>) -> Self {
        Self {
            events: Mutex::new(events.into()),
            closes: AtomicUsize::new(0),
        }
    }

    /// How many times the worker asked the source to shut down.
    pub fn closes(&self) -> usize {
        self.closes.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IdleSource for ScriptedIdle {
    async fn wait(&self, _shutdown: &mut Rx) -> Result<IdleEvent, IdleError> {
        let mut events = self.events.lock().expect("scripted idle lock poisoned");
        match events.pop_front() {
            Some(outcome) => outcome,
            None => panic!("scripted idle exhausted after {} waits", self.closes()),
        }
    }

    async fn close(&self) {
        self.closes.fetch_add(1, Ordering::SeqCst);
    }
}

/// The usual first entry: "something arrived".
pub fn new_mail() -> Result<IdleEvent, IdleError> {
    Ok(IdleEvent::NewMail)
}

/// The usual middle entry: "nothing arrived, re-issue".
pub fn timeout() -> Result<IdleEvent, IdleError> {
    Ok(IdleEvent::Timeout)
}

/// The usual last entry: "the process is going down".
pub fn stopped() -> Result<IdleEvent, IdleError> {
    Ok(IdleEvent::Stopped)
}

/// A failure the worker is expected to retry after a backoff.
pub fn transient(message: &str) -> Result<IdleEvent, IdleError> {
    Err(IdleError::Transient(crate::error::MailError::Unavailable(
        message.to_string(),
    )))
}

/// A failure the worker is expected to stop on.
pub fn unsupported(message: &str) -> Result<IdleEvent, IdleError> {
    Err(IdleError::Unsupported(crate::error::MailError::Auth(
        message.to_string(),
    )))
}
