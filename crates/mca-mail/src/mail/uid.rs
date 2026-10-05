//! UID-based fresh-mail detection, shared by the transports and the poller.
//!
//! Newness is a property of the high-water mark, not of the `\Seen` flag: a
//! read-only deployment cannot set flags, and a flag-driven scan re-reads
//! history after every restart.

use crate::domain::InboundMessage;

/// Server-side UID state of the selected mailbox, read fresh each cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UidState {
    /// UIDVALIDITY of the mailbox; a change invalidates every stored UID.
    pub uid_validity: u32,
    /// RFC 3501 UIDNEXT: the UID the next arriving message will receive.
    pub uid_next: u32,
}

impl UidState {
    /// The boundary for a first run: every message currently on the server has
    /// a UID below `uid_next`, so none of them counts as new afterwards.
    ///
    /// `saturating_sub` matters: a misreporting mailbox must never yield a
    /// boundary that puts existing mail on the new side of it.
    pub fn high_edge(&self) -> u32 {
        self.uid_next.saturating_sub(1)
    }
}

/// One watermark-bounded fetch: messages that arrived after the boundary, plus
/// the highest UID the transport examined to find them.
///
/// `highest_uid` covers messages that did not survive parsing or the size
/// guard as well: the caller advances the watermark to it so a poison message
/// cannot block the batch queued behind it. `None` means nothing matched.
#[derive(Debug, Default)]
pub struct FetchBatch {
    pub messages: Vec<InboundMessage>,
    pub highest_uid: Option<u32>,
}
