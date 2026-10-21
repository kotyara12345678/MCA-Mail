//! Mailbox poll cycle: fresh-mail boundary, fetch, dedupe, store.
//!
//! What counts as new is decided by the cursor (UIDVALIDITY + high-water
//! mark), never by the `\Seen` flag: a read-only deployment cannot set flags,
//! and a flag-driven scan re-reads the whole history after every restart.

use std::time::Instant;

use sqlx::PgPool;
use tracing::info;

use super::cursor::{self, Boundary};
use crate::domain::InboundMessage;
use crate::error::AppError;
use crate::mail::MailProvider;
use crate::observability::{lifecycle, queue};
use crate::persistence::{email_repo, settings_repo, thread_repo};

/// One poll cycle: establish the boundary, fetch only mail after it, store it,
/// and only then advance the boundary. Processing is left to the queue loop so
/// a slow LLM never blocks fetching.
///
/// The loop that decides *when* to read lives in [`super::sync`]; this function
/// only decides how. Public so the integration tests can drive it directly
/// against a test database.
pub async fn poll_once(
    provider: &dyn MailProvider,
    pool: &PgPool,
    mailbox: &str,
) -> Result<(), AppError> {
    // Admin panel pause: no fetch, no cursor movement — the cycle is skipped
    // as if it never fired, so nothing is lost when the switch is flipped back.
    if settings_repo::is_paused(pool, settings_repo::POLL_PAUSED).await {
        info!(mailbox, "fetch paused by admin; skipping cycle");
        return Ok(());
    }

    queue::poll_started(mailbox);
    let started = Instant::now();

    let state = provider.uid_state().await.map_err(AppError::Mail)?;
    let after = match cursor::boundary(pool, mailbox, &state).await? {
        Boundary::Hold(reason) => {
            info!(mailbox, reason, "no fetch this cycle");
            queue::poll_completed(mailbox, started.elapsed().as_millis() as u64, 0);
            return Ok(());
        }
        Boundary::Fetch(after) => after,
    };

    let batch = provider.fetch_after(after).await.map_err(AppError::Mail)?;
    let fetched = batch.messages.len() as i64;
    queue::emails_fetched(fetched);
    let (inserted, duplicates) =
        store_batch(pool, mailbox, i64::from(state.uid_validity), batch.messages).await?;
    if inserted > 0 {
        queue::emails_inserted(inserted);
    }
    if duplicates > 0 {
        queue::emails_skipped_duplicate(duplicates);
    }
    // The boundary moves only after every message is durably stored. A crash
    // before this line re-reads the same range next cycle and the dedup key
    // absorbs the replay; a crash after it cannot lose anything already stored.
    cursor::commit(pool, mailbox, &state, batch.highest_uid).await?;
    queue::poll_completed(mailbox, started.elapsed().as_millis() as u64, fetched);
    Ok(())
}

/// Store one fetched batch; returns `(inserted, duplicates)`.
async fn store_batch(
    pool: &PgPool,
    mailbox: &str,
    uid_validity: i64,
    messages: Vec<InboundMessage>,
) -> Result<(i64, i64), AppError> {
    let mut inserted: i64 = 0;
    let mut duplicates: i64 = 0;
    for message in messages {
        // Resolve the conversation thread: replies chain by reference, new
        // messages by (sender, normalized subject).
        let thread_id = resolve_thread(pool, mailbox, &message).await?;
        let outcome =
            email_repo::insert_inbound(pool, thread_id, mailbox, &message, Some(uid_validity))
                .await?;
        match outcome {
            email_repo::InsertOutcome::Inserted(id) => {
                info!(email_id = %id, "stored new message");
                lifecycle::email_received(id, thread_id, message.internet_message_id.as_deref());
                inserted += 1;
            }
            email_repo::InsertOutcome::Duplicate(id) => {
                info!(email_id = %id, "message already stored, skipping");
                duplicates += 1;
            }
        }
    }
    Ok((inserted, duplicates))
}

async fn resolve_thread(
    pool: &PgPool,
    _mailbox: &str,
    message: &InboundMessage,
) -> Result<crate::domain::ThreadId, AppError> {
    // Chain onto an existing thread by Message-ID references. `In-Reply-To`
    // counts too: plenty of clients answer with that header alone, and a
    // missed chain would spawn a fresh thread — and a fresh lead — for a
    // reply inside the same conversation.
    let candidates = reference_candidates(message);
    if let Some(thread) = thread_repo::find_by_reference(pool, &candidates).await? {
        return Ok(thread);
    }
    // Otherwise use the stable conversation key.
    let participants = [&message.from];
    let normalized = crate::domain::normalize_subject(&message.subject);
    let key = thread_repo::conversation_key(&participants, &normalized);
    let thread = thread_repo::ensure_thread(pool, &key, &normalized, None).await?;
    Ok(thread)
}

/// Lookup order: the direct parent first (most precise), then the rest of the
/// reference chain as sent by the client, without repeating the parent.
fn reference_candidates(message: &InboundMessage) -> Vec<String> {
    let mut candidates = Vec::with_capacity(message.references.len() + 1);
    if let Some(parent) = message.in_reply_to.as_deref() {
        let parent = parent.trim();
        if !parent.is_empty() {
            candidates.push(parent.to_string());
        }
    }
    for reference in &message.references {
        let reference = reference.trim();
        if !reference.is_empty() && !candidates.iter().any(|c| c == reference) {
            candidates.push(reference.to_string());
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::reference_candidates;
    use crate::domain::{EmailAddress, InboundMessage};

    fn message(in_reply_to: Option<&str>, references: &[&str]) -> InboundMessage {
        InboundMessage {
            provider_message_id: "1".into(),
            internet_message_id: Some("<one@x>".into()),
            in_reply_to: in_reply_to.map(str::to_string),
            references: references.iter().map(|r| r.to_string()).collect(),
            from: EmailAddress::new("client@example.com"),
            to: vec![EmailAddress::new("info@mca-log.com")],
            cc: vec![],
            subject: "Re: Quote".into(),
            date: None,
            text_body: "body".into(),
            attachments: vec![],
            total_size: 4,
        }
    }

    /// The direct parent wins the lookup, and an `In-Reply-To`-only reply
    /// still chains instead of falling back to the subject key.
    #[test]
    fn in_reply_to_is_a_candidate_before_the_reference_chain() {
        let both = message(Some("<parent@x>"), &["<root@x>", " <parent@x> "]);
        let candidates = reference_candidates(&both);
        assert_eq!(candidates[0], "<parent@x>");
        assert_eq!(candidates[1], "<root@x>");
        assert_eq!(candidates.len(), 2, "duplicate parent is not a second key");

        let parent_only = message(Some("<parent@x>"), &[]);
        assert_eq!(reference_candidates(&parent_only), ["<parent@x>"]);

        let blank = message(Some("  "), &["", "<root@x>"]);
        assert_eq!(reference_candidates(&blank), ["<root@x>"]);
    }
}
