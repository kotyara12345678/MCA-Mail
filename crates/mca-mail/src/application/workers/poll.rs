//! Mailbox poll cycle: fetch, dedupe, store (processing stays on the queue).

use std::sync::Arc;
use std::time::Instant;

use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{error, info};

use crate::error::AppError;
use crate::mail::MailProvider;
use crate::observability::{lifecycle, queue};
use crate::orchestration::Orchestrator;
use crate::persistence::{email_repo, thread_repo};

/// Poll the configured mailbox on a schedule and hand new messages to the
/// orchestrator.
pub async fn mail_poll_loop(
    provider: Arc<dyn MailProvider>,
    pool: PgPool,
    orchestrator: Arc<Orchestrator>,
    poll_seconds: u64,
    mailbox: String,
) {
    let mut ticker = interval(Duration::from_secs(poll_seconds.max(5)));
    info!(mailbox = %mailbox, poll_seconds, "mail poll loop started");

    loop {
        ticker.tick().await;
        if let Err(e) = poll_once(provider.as_ref(), &pool, &orchestrator, &mailbox).await {
            error!(error = %e, "mail poll cycle failed");
        }
    }
}

/// One poll cycle: fetch, dedupe, store. Processing is left to the queue
/// loop so a slow LLM never blocks fetching.
async fn poll_once(
    provider: &dyn MailProvider,
    pool: &PgPool,
    _orchestrator: &Orchestrator,
    mailbox: &str,
) -> Result<(), AppError> {
    queue::poll_started(mailbox);
    let started = Instant::now();

    let messages = provider
        .fetch_new()
        .await
        .map_err(crate::error::AppError::Mail)?;
    let fetched = messages.len() as i64;
    queue::emails_fetched(fetched);

    let mut inserted: i64 = 0;
    let mut duplicates: i64 = 0;
    for message in messages {
        // Resolve the conversation thread: replies chain by reference, new
        // messages by (sender, normalized subject).
        let thread_id = resolve_thread(pool, mailbox, &message).await?;
        let outcome = email_repo::insert_inbound(pool, thread_id, mailbox, &message).await?;

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
    if inserted > 0 {
        queue::emails_inserted(inserted);
    }
    if duplicates > 0 {
        queue::emails_skipped_duplicate(duplicates);
    }
    queue::poll_completed(mailbox, started.elapsed().as_millis() as u64, fetched);
    Ok(())
}

async fn resolve_thread(
    pool: &PgPool,
    _mailbox: &str,
    message: &crate::domain::InboundMessage,
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
