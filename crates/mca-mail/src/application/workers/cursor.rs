//! The fresh-mail boundary: what this poll cycle is allowed to read.
//!
//! One row in `mailbox_cursors` per mailbox holds UIDVALIDITY and the
//! high-water mark. The row is written *before* any fetch on the first cycle
//! and after every fully stored batch, so restarts resume where they stopped
//! and a UIDVALIDITY change re-bounds the mailbox instead of replaying it.

use sqlx::PgPool;
use tracing::{info, warn};

use crate::error::AppError;
use crate::mail::UidState;
use crate::persistence::cursor_repo::{self, MailboxCursor};

/// What a poll cycle may do after consulting the cursor.
pub(super) enum Boundary {
    /// Record a boundary only — no fetch this cycle.
    Hold(&'static str),
    /// Fetch UIDs strictly greater than this one.
    Fetch(u32),
}

/// Decide the boundary for this cycle, recording a new one when due.
pub(super) async fn boundary(
    pool: &PgPool,
    mailbox: &str,
    state: &UidState,
) -> Result<Boundary, AppError> {
    match plan(cursor_repo::load(pool, mailbox).await?.as_ref(), state) {
        Plan::Initialize(reason, edge) => {
            cursor_repo::initialize(pool, mailbox, state.uid_validity, edge).await?;
            info!(
                mailbox,
                uid_validity = state.uid_validity,
                edge,
                reason,
                "boundary recorded"
            );
            Ok(Boundary::Hold(reason))
        }
        Plan::Reset(reason, edge) => {
            cursor_repo::reset(pool, mailbox, state.uid_validity, edge).await?;
            warn!(
                mailbox,
                uid_validity = state.uid_validity,
                edge,
                reason,
                "boundary reset"
            );
            Ok(Boundary::Hold(reason))
        }
        Plan::Continue(after) => Ok(Boundary::Fetch(after)),
    }
}

/// Persist progress after a fully stored batch: forward-only, same validity.
///
/// A no-op result is not an error: it means another writer re-bound the
/// mailbox or the mark is already ahead, and the next cycle re-reads the
/// cursor anyway.
pub(super) async fn commit(
    pool: &PgPool,
    mailbox: &str,
    state: &UidState,
    highest: Option<u32>,
) -> Result<(), AppError> {
    let Some(highest) = highest else {
        return Ok(());
    };
    if !cursor_repo::advance(pool, mailbox, state.uid_validity, highest).await? {
        tracing::debug!(mailbox, highest, "boundary not advanced");
    }
    Ok(())
}

/// The decision itself, separated from the writes so it can be tested directly.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    Initialize(&'static str, u32),
    Reset(&'static str, u32),
    Continue(u32),
}

fn plan(cursor: Option<&MailboxCursor>, state: &UidState) -> Plan {
    match cursor {
        None => Plan::Initialize(
            "first run: existing mail is bounded, not processed",
            state.high_edge(),
        ),
        Some(stored) if stored.uid_validity != i64::from(state.uid_validity) => Plan::Reset(
            "UIDVALIDITY changed: re-bounded without backfill",
            state.high_edge(),
        ),
        Some(stored) => Plan::Continue(clamp(stored.high_water_uid)),
    }
}

/// A stored mark beyond `u32::MAX` is not a UID this transport can fetch;
/// clamping to the maximum fetches nothing rather than wrapping to zero.
fn clamp(high_water: i64) -> u32 {
    u32::try_from(high_water).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
