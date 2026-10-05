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
use crate::persistence::{email_repo, thread_repo};

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
    // Try to chain onto an existing thread by Message-ID references.
    if let Some(thread) = thread_repo::find_by_reference(pool, &message.references).await? {
        return Ok(thread);
    }
    // Otherwise use the stable conversation key.
    let participants = [&message.from];
    let normalized = crate::domain::normalize_subject(&message.subject);
    let key = thread_repo::conversation_key(&participants, &normalized);
    let thread = thread_repo::ensure_thread(pool, &key, &normalized, None).await?;
    Ok(thread)
}
