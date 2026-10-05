use async_imap::types::Fetch;
use futures::StreamExt;

use super::ImapMailProvider;
use crate::domain::InboundMessage;
use crate::error::MailError;
use crate::mail::MailboxWriter;

impl ImapMailProvider {
    /// Search, order, bound and fetch one batch of message bodies.
    ///
    /// `async-imap` hands back a `HashSet`, so the server's own order is lost
    /// and a bare `take(limit)` on it would select an arbitrary slice of the
    /// mailbox. [`select_batch`] sorts before the limit, which makes the batch
    /// the *oldest* matching mail — the property the high-water mark depends
    /// on.
    ///
    /// Bodies come from `BODY.PEEK[]`, so nothing is marked seen: a crash
    /// between fetch and database commit cannot silently lose a message.
    /// Returns the raw messages plus the highest UID examined (even when a
    /// body was later skipped), so the caller can advance the boundary past
    /// poison mail instead of re-reading it for ever.
    pub(super) async fn fetch_uid_range(
        &self,
        query: &str,
        after_uid: u32,
        limit: usize,
    ) -> Result<(Vec<(u32, Vec<u8>)>, Option<u32>), MailError> {
        let query = query.to_string();
        let session_flags_allowed = self.guard().allows_write();
        self.with_session(move |session| {
            Box::pin(async move {
                let found = session.uid_search(&query).await.map_err(map_imap_error)?;
                let selected = select_batch(found, after_uid, limit);
                let highest = selected.last().copied();
                if selected.is_empty() {
                    return Ok((Vec::new(), None));
                }
                let keys: Vec<String> = selected.iter().map(u32::to_string).collect();
                // The fetch stream borrows the session for its whole lifetime, so
                // bodies are collected before any further command is issued.
                let (accepted, oversized) = {
                    let mut messages = session
                        .uid_fetch(keys.join(","), "(UID RFC822.SIZE BODY.PEEK[])")
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
                        if raw.len() > crate::mail::parse::MAX_MESSAGE_BYTES {
                            oversized.push(uid);
                            continue;
                        }
                        accepted.push((uid, raw.to_vec()));
                    }
                    (accepted, oversized)
                };

                // An oversized message is skipped by policy, not retried: the
                // watermark advances past every examined UID, so the size guard
                // must decide here or the message would be lost silently.
                // Flagging is a mutation, so under read-only only the refusal
                // is recorded; under `work` the flag documents it for operators.
                for uid in oversized {
                    tracing::warn!(uid, "inbound message exceeds size guard, skipped");
                    if !session_flags_allowed {
                        continue;
                    }
                    // The mode was already checked above, so this store is allowed.
                    // Should the guard ever gain a second condition, a refusal here
                    // is logged and the skip still stands.
                    match mark_flagged(session, uid, "\\Seen").await {
                        Ok(()) => {}
                        Err(refusal) => tracing::info!(uid, %refusal, "flag not set"),
                    }
                }
                Ok((accepted, highest))
            })
        })
        .await
    }

    /// Parse a raw batch into domain messages, quarantining what will not parse.
    ///
    /// The quarantine call is guarded like every other mutation, so a read-only
    /// deployment records the reason and moves on instead of touching the server.
    pub(super) async fn parse_batch(
        &self,
        raw_messages: Vec<(u32, Vec<u8>)>,
    ) -> Result<Vec<InboundMessage>, MailError> {
        let mut out = Vec::with_capacity(raw_messages.len());
        for (uid, raw) in raw_messages {
            match crate::mail::parse::parse(&raw, uid.to_string()) {
                Ok(message) => out.push(message),
                Err(error) => {
                    // Unparseable mail must not block the batch: record why and
                    // continue. Quarantine is a mutation, so it goes through the
                    // guarded trait method: calling the inherent impl here would
                    // bypass the read-only guard and move mail on the server.
                    tracing::warn!(uid, %error, "skipping unparseable message");
                    if let Err(refusal) = MailboxWriter::quarantine(self, uid, "unparseable").await
                    {
                        tracing::info!(uid, %refusal, "quarantine skipped");
                    }
                }
            }
        }
        Ok(out)
    }
}

pub(super) async fn mark_flagged(
    session: &mut super::ImapSession,
    uid: u32,
    flag: &str,
) -> Result<(), MailError> {
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

/// Cut a search result down to `uid > after_uid`, oldest first, `limit` max.
///
/// Pure so the ordering guarantee is testable without a server: for any
/// iteration order of the `HashSet` the batch must be the same oldest slice.
fn select_batch(found: std::collections::HashSet<u32>, after_uid: u32, limit: usize) -> Vec<u32> {
    let mut uids: Vec<u32> = found.into_iter().filter(|&uid| uid > after_uid).collect();
    uids.sort_unstable();
    uids.truncate(limit);
    uids
}

/// Shared by every command that turns a wire-level failure into a `MailError`.
///
/// `pub(crate)` rather than module-private because the IDLE watcher, which lives
/// outside this module, has to classify failures from its own commands too.
pub(crate) fn map_imap_error(error: async_imap::error::Error) -> MailError {
    match error {
        async_imap::error::Error::ConnectionLost => {
            MailError::Unavailable("imap connection lost".into())
        }
        // `NO` covers both a rejected command and a bad login; the message text
        // is preserved so the operator can tell them apart in the log.
        other => MailError::Protocol(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::select_batch;

    /// `UID SEARCH` returns a `HashSet`; every iteration order must produce
    /// the same oldest-first batch, or the batch limit would pick a random
    /// slice of history instead of the oldest new mail.
    #[test]
    fn batch_is_sorted_oldest_first_regardless_of_hash_order() {
        for _ in 0..16 {
            let found: HashSet<u32> = [7, 1, 9, 3, 5].into_iter().collect();
            assert_eq!(select_batch(found, 0, 3), vec![1, 3, 5]);
        }
    }

    #[test]
    fn boundary_excludes_uids_at_or_below_the_mark() {
        let found: HashSet<u32> = [4, 5, 6].into_iter().collect();
        assert_eq!(select_batch(found, 4, 10), vec![5, 6]);
    }

    #[test]
    fn limit_keeps_the_oldest_slice() {
        let found: HashSet<u32> = (1..=9).collect();
        assert_eq!(select_batch(found, 0, 2), vec![1, 2]);
    }

    #[test]
    fn empty_search_yields_an_empty_batch() {
        assert!(select_batch(HashSet::new(), 42, 10).is_empty());
    }
}
