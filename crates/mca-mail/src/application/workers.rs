//! Background workers: periodic mail polling, processing, retention.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

use crate::config::AppConfig;
use crate::error::AppError;
use crate::mail::MailProvider;
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

/// On a single-instance start, any run still marked `running` is orphaned by
/// the previous process. Fail them first, then reset their emails to
/// `pending` so the pipeline resumes from the recorded stage.
pub async fn recover_stuck_emails(pool: &PgPool) -> Result<u64, AppError> {
    let runs = sqlx::query(
        "UPDATE email_processing_runs SET state = 'failed', finished_at = now() \
         WHERE state = 'running'",
    )
    .execute(pool)
    .await?
    .rows_affected();
    let emails = sqlx::query(
        "UPDATE emails SET status = 'pending', last_error = 'interrupted by restart' \
         WHERE status = 'processing'",
    )
    .execute(pool)
    .await?
    .rows_affected();
    if emails > 0 || runs > 0 {
        info!(orphan_runs = runs, emails, "recovered interrupted emails");
    }
    Ok(emails)
}

/// One poll cycle: fetch, dedupe, store. Processing is left to the queue
/// loop so a slow LLM never blocks fetching.
async fn poll_once(
    provider: &dyn MailProvider,
    pool: &PgPool,
    _orchestrator: &Orchestrator,
    mailbox: &str,
) -> Result<(), AppError> {
    let messages = provider
        .fetch_new()
        .await
        .map_err(crate::error::AppError::Mail)?;

    if messages.is_empty() {
        return Ok(());
    }

    info!(count = messages.len(), "fetched new messages");

    for message in messages {
        // Resolve the conversation thread: replies chain by reference, new
        // messages by (sender, normalized subject).
        let thread_id = resolve_thread(pool, mailbox, &message).await?;

        let outcome = email_repo::insert_inbound(pool, thread_id, mailbox, &message).await?;

        match outcome {
            email_repo::InsertOutcome::Inserted(id) => {
                info!(email_id = %id, "stored new message");
            }
            email_repo::InsertOutcome::Duplicate(id) => {
                info!(email_id = %id, "message already stored, skipping");
            }
        }
    }

    Ok(())
}

/// Claim and process pending emails, `concurrency` at a time.
///
/// `claim_batch` flips rows to `processing` under `FOR UPDATE SKIP LOCKED`, so
/// several queue loops can run without double-processing, and `process_email`
/// accepts the already-claimed `processing` state.
pub async fn queue_loop(pool: PgPool, orchestrator: Arc<Orchestrator>) {
    let concurrency: i64 = 4;
    let max_attempts: i32 = 3;
    let mut ticker = interval(Duration::from_secs(2));
    info!(concurrency, "queue loop started");

    loop {
        ticker.tick().await;
        let batch = match email_repo::claim_batch(&pool, concurrency, max_attempts).await {
            Ok(b) if b.is_empty() => continue,
            Ok(b) => b,
            Err(e) => {
                error!(error = %e, "claim_batch failed");
                continue;
            }
        };

        let tasks = batch.into_iter().map(|row| {
            let orch = orchestrator.clone();
            async move {
                if let Err(e) = orch.process_email(row.id).await {
                    warn!(email_id = %row.id, error = %e, "queue processing failed");
                }
            }
        });
        futures::future::join_all(tasks).await;
    }
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

/// Periodic retention job (email bodies anonymization, event purge).
pub async fn retention_loop(pool: PgPool, settings: crate::config::RetentionSettings) {
    let mut ticker = interval(Duration::from_secs(
        (settings.check_interval_hours.max(1) as u64) * 3600,
    ));
    info!("retention loop started");

    loop {
        ticker.tick().await;
        match crate::persistence::retention_repo::run(&pool, &settings).await {
            Ok(report) => {
                info!(report = ?report, "retention cycle completed");
            }
            Err(e) => {
                error!(error = %e, "retention cycle failed");
            }
        }
    }
}

/// Start all background loops.
pub async fn spawn_all(config: &AppConfig, pool: PgPool, orchestrator: Arc<Orchestrator>) {
    let provider = match crate::mail::build(&config.mail) {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "failed to build mail provider; poll loop disabled");
            return;
        }
    };
    let provider: Arc<dyn MailProvider> = provider.into();

    if let Err(e) = provider.init().await {
        error!(error = %e, "failed to initialise mail provider; poll loop disabled");
        return;
    }
    info!(provider = %provider.name(), "mail provider initialised");

    if let Err(e) = recover_stuck_emails(&pool).await {
        error!(error = %e, "failed to recover interrupted emails");
    }

    let mailbox = config.mail.inbox().to_string();
    let poll_seconds = config.mail.poll_interval_seconds;

    tokio::spawn(mail_poll_loop(
        provider,
        pool.clone(),
        orchestrator.clone(),
        poll_seconds,
        mailbox,
    ));

    tokio::spawn(queue_loop(pool.clone(), orchestrator));

    if config.retention.enabled {
        tokio::spawn(retention_loop(pool, config.retention.clone()));
    }
}
