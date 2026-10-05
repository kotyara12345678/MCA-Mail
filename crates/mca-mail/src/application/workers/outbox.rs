//! Stage E: the one path from "queued" to "delivered".
//!
//! Application code never touches SMTP. It writes a row into `mailbox_outbox`
//! and this worker decides, at the moment of sending, whether policy still
//! permits it. That indirection is the point: flipping `EMAIL_MODE` to
//! `dry_run` stops delivery for messages that were already queued, because the
//! check runs at send time rather than at enqueue time.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

use crate::config::AppConfig;
use crate::domain::{EmailAddress, OutboundMessage};
use crate::mail::{
    MailboxWriter, MaybeWritable, OutboundDecision, OutboundPolicyGuard, PolicyContext,
    PolicyDenial,
};
use crate::persistence::{
    conversation_repo, draft_repo, lead_repo, outbox_repo, outbox_repo::OutboundKind, settings_repo,
};
use crate::shutdown::{stopping, Rx};

/// How many queued sends are examined per tick.
const BATCH: i64 = 8;
/// Backoff for a transport failure, doubled per attempt and capped so a
/// permanently broken SMTP server does not silence a lead for a day.
const RETRY_BASE_SECONDS: i64 = 5;
const RETRY_MAX_SECONDS: i64 = 300;
/// How long to wait when a pacing limit is what said no.
const PACE_WAIT_SECONDS: i64 = 60;
/// A claim older than this belongs to a process that is no longer running.
const CLAIM_TIMEOUT_SECONDS: i64 = 300;

pub async fn outbox_loop(
    pool: PgPool,
    writer: Arc<dyn MaybeWritable>,
    config: AppConfig,
    mut shutdown: Rx,
) {
    let mut ticker = interval(Duration::from_secs(1));
    info!(mode = %config.security.email_mode.as_str(), "outbox loop started");

    loop {
        tokio::select! {
            _ = stopping(&mut shutdown) => break,
            _ = ticker.tick() => {}
        }
        // A row left in `sending` by a process that died belongs back in the
        // queue; without this it would sit there for ever.
        match outbox_repo::release_stale(&pool, CLAIM_TIMEOUT_SECONDS).await {
            Ok(n) if n > 0 => warn!(released = n, "recovered abandoned sends"),
            Ok(_) => {}
            Err(e) => error!(error = %e, "failed to recover abandoned sends"),
        }

        let batch = match outbox_repo::claim_due(&pool, BATCH).await {
            Ok(b) if b.is_empty() => continue,
            Ok(b) => b,
            Err(e) => {
                error!(error = %e, "outbox claim failed");
                continue;
            }
        };
        for row in batch {
            if let Err(e) = deliver(&pool, &writer, &config, row).await {
                error!(error = %e, "outbox delivery failed");
            }
        }
    }
    info!("outbox loop stopped");
}

async fn deliver(
    pool: &PgPool,
    writer: &Arc<dyn MaybeWritable>,
    config: &AppConfig,
    row: outbox_repo::OutboundRow,
) -> Result<(), crate::error::AppError> {
    let id = row.id;
    let kind = row.kind()?;
    let attempts = row.attempts;

    if row.recipient.trim().is_empty() {
        outbox_repo::mark_held(pool, id, "no_recipient", "recipient is empty").await?;
        return Ok(());
    }

    // The manager card has its own switch and its own ceiling: it must not
    // consume the customer-reply quota, and turning it off must not require
    // touching the general outbound policy.
    if kind == OutboundKind::ManagerCard {
        let card = &config.security.manager_card;
        if !card.enabled {
            outbox_repo::mark_held(pool, id, "manager_card_disabled", "SEND_MANAGER_CARD=false")
                .await?;
            return Ok(());
        }
        let sent = outbox_repo::manager_cards_last_hour(pool, &row.recipient).await?;
        if sent >= i64::from(card.max_per_hour) {
            outbox_repo::reschedule(pool, id, "manager_card_rate_limit", PACE_WAIT_SECONDS).await?;
            return Ok(());
        }
    }

    let context = policy_context(pool, config, &row).await?;
    let guard = OutboundPolicyGuard::from_config(config);

    match guard.evaluate(&context) {
        OutboundDecision::Send => {}
        OutboundDecision::Draft(denial) | OutboundDecision::Suppress(denial) => {
            apply_denial(pool, id, &denial).await?;
            return Ok(());
        }
    }

    let message = match to_message(&row) {
        Ok(m) => m,
        Err(e) => {
            outbox_repo::mark_held(pool, id, "invalid_recipient", &e.to_string()).await?;
            return Ok(());
        }
    };

    match writer.send(&message).await {
        Ok(provider_id) => {
            outbox_repo::mark_sent(pool, id, Some(&provider_id)).await?;
            if let Some(lead_id) = row.lead_id {
                // The outbox idempotency key of a reply is the conversation
                // entry's key, so the history flips to `sent` exactly when the
                // message actually left.
                conversation_repo::mark_sent(
                    pool,
                    lead_id,
                    row.idempotency_key.as_deref().unwrap_or_default(),
                )
                .await?;
            }
            // So does the draft the reply was built from. Nobody else flips
            // it: auto-send has no reviewer, so a row left at
            // `pending_approval` would keep winning `live_draft_for_lead` and
            // the lead's next reply would collapse onto it instead of going.
            if kind == OutboundKind::CustomerReply {
                if let Some(draft_id) = row
                    .idempotency_key
                    .as_deref()
                    .and_then(draft_repo::draft_id_from_outbound_key)
                {
                    draft_repo::mark_sent(pool, draft_id, &provider_id).await?;
                }
            }
            info!(outbox_id = %id, kind = kind.as_str(), "message delivered");
            // Keep a copy where the sender can see it went out. Bookkeeping,
            // not delivery: the message is already gone, so failing here is a
            // warning — reporting it as a delivery failure would make the next
            // attempt send the same message twice.
            match writer.append_sent(&message).await {
                Ok(()) => info!(outbox_id = %id, "archived in the sent folder"),
                Err(e) => warn!(
                    outbox_id = %id,
                    error = %e,
                    "delivered, but not archived in the sent folder"
                ),
            }
        }
        Err(e) => {
            let delay = backoff_seconds(attempts);
            warn!(
                outbox_id = %id,
                attempts,
                delay,
                error = %e,
                "smtp delivery failed; will retry"
            );
            outbox_repo::mark_retry(
                pool,
                id,
                attempts + 1,
                row.max_attempts,
                &e.to_string(),
                delay,
            )
            .await?;
        }
    }
    Ok(())
}

/// Everything the outbound policy needs to say yes or no, read fresh, because
/// a queued message may have been waiting while the answer changed.
async fn policy_context(
    pool: &PgPool,
    config: &AppConfig,
    row: &outbox_repo::OutboundRow,
) -> Result<PolicyContext, crate::error::AppError> {
    let stats = outbox_repo::send_stats(pool, &row.recipient).await?;
    let mut context = PolicyContext::denied_by_default(
        config.security.email_mode,
        &row.recipient,
        chrono::Utc::now(),
    )
    .with_sends(stats.recipient_last_hour, stats.total_last_hour)
    .with_body(row.body_text.chars().count());
    context.lead_id = row.lead_id;
    context.last_sent_at = stats.last_sent_at;

    if let Some(lead_id) = row.lead_id {
        context.consecutive_replies = conversation_repo::unanswered_replies(pool, lead_id).await?;
        context.automation_locked = lead_repo::is_automation_locked(pool, lead_id).await?;
    }
    context.automation_enabled = settings_repo::automation_enabled(pool).await?;
    Ok(context)
}

/// A denial that time can cure is a delay; one that cannot is a hold.
async fn apply_denial(
    pool: &PgPool,
    id: uuid::Uuid,
    denial: &PolicyDenial,
) -> Result<(), crate::error::AppError> {
    let reason = denial.as_str();
    match denial {
        PolicyDenial::MinIntervalNotElapsed { wait_seconds } => {
            outbox_repo::reschedule(pool, id, reason, (*wait_seconds).max(1)).await?;
        }
        PolicyDenial::GlobalRateLimit { .. }
        | PolicyDenial::RecipientRateLimit { .. }
        | PolicyDenial::TooManyConsecutiveReplies { .. } => {
            outbox_repo::reschedule(pool, id, reason, PACE_WAIT_SECONDS).await?;
        }
        _ => {
            // Mode, approval, body size, recipient: waiting will not change the
            // answer, so the row is kept with its reason and never retried.
            outbox_repo::mark_held(pool, id, reason, reason).await?;
        }
    }
    Ok(())
}

fn backoff_seconds(attempts: i32) -> i64 {
    let shift = attempts.clamp(0, 10);
    (RETRY_BASE_SECONDS << shift).min(RETRY_MAX_SECONDS)
}

fn to_message(row: &outbox_repo::OutboundRow) -> Result<OutboundMessage, crate::error::AppError> {
    let to = EmailAddress::new(&row.recipient);
    if !to.address.contains('@') {
        return Err(crate::error::AppError::InvalidInput(format!(
            "queued message has no usable recipient: {}",
            crate::observability::errors::clip(&row.recipient, 80)
        )));
    }
    Ok(OutboundMessage {
        to: vec![to],
        cc: Vec::new(),
        subject: row.subject.clone(),
        text_body: row.body_text.clone(),
        html_body: (!row.body_html.is_empty()).then(|| row.body_html.clone()),
        in_reply_to: row.in_reply_to.clone(),
        references: row.ref_headers.clone(),
        attachments: Vec::new(),
    })
}
