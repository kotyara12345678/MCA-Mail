//! Pure boundary decisions: first run, UIDVALIDITY change, steady state.

use super::{plan, Plan};
use crate::mail::UidState;
use crate::persistence::cursor_repo::MailboxCursor;

fn state(uid_validity: u32, uid_next: u32) -> UidState {
    UidState {
        uid_validity,
        uid_next,
    }
}

fn stored(uid_validity: i64, high_water_uid: i64) -> MailboxCursor {
    MailboxCursor {
        uid_validity,
        high_water_uid,
    }
}

fn kind(plan: &Plan) -> &'static str {
    match plan {
        Plan::Initialize(..) => "initialize",
        Plan::Reset(..) => "reset",
        Plan::Continue(_) => "continue",
    }
}

/// First run: the boundary sits at UIDNEXT-1, so every message already on the
/// server is old — none of it may be fetched, let alone processed.
#[test]
fn first_run_bounds_at_uid_next_without_processing_history() {
    let plan = plan(None, &state(7, 15_000));
    assert_eq!(kind(&plan), "initialize");
    if let Plan::Initialize(_, edge) = plan {
        assert_eq!(edge, 14_999);
    }
}

/// An empty mailbox reports UIDNEXT 1: the boundary is 0, which is safe
/// precisely because there is nothing to backfill.
#[test]
fn first_run_on_an_empty_mailbox_bounds_at_zero() {
    let plan = plan(None, &state(7, 1));
    assert_eq!(
        plan,
        Plan::Initialize("first run: existing mail is bounded, not processed", 0)
    );
}

/// A validity change re-bounds at the current edge: messages that exist under
/// the new numbering are old by definition, so no backfill can start.
#[test]
fn uidvalidity_change_resets_instead_of_fetching() {
    let plan = plan(Some(&stored(1, 42)), &state(2, 100));
    assert_eq!(kind(&plan), "reset");
    if let Plan::Reset(_, edge) = plan {
        assert_eq!(edge, 99);
    }
}

/// Steady state: the stored mark wins, no matter what UIDNEXT says — new mail
/// beyond the mark is what `Fetch` will read.
#[test]
fn matching_validity_continues_from_the_stored_mark() {
    assert_eq!(
        plan(Some(&stored(7, 1_234)), &state(7, 9_999)),
        Plan::Continue(1_234)
    );
}

/// A mark beyond the transport's UID space must clamp to "fetch nothing"
/// instead of wrapping around to the beginning of the mailbox.
#[test]
fn absurd_stored_mark_clamps_to_fetch_nothing() {
    assert_eq!(
        plan(Some(&stored(7, i64::MAX)), &state(7, 50)),
        Plan::Continue(u32::MAX)
    );
}
