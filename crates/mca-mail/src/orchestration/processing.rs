//! End-to-end processing of one claimed email, with lifecycle events.

use tracing::{error, info, warn};

use super::Orchestrator;
use crate::domain::{EmailStatus, RunState, RunTrigger};
use crate::error::AppError;
use crate::observability::{errors, lifecycle, Correlation};
use crate::persistence::{email_repo, run_repo, thread_repo};

impl Orchestrator {
    /// Process a single email through the full pipeline.
    ///
    /// Accepts both `pending` (fresh poll) and `processing` (claimed by the
    /// queue worker via `claim_batch`, which is atomic); anything already
    /// finished is skipped.
    pub async fn process_email(&self, email_id: uuid::Uuid) -> Result<(), AppError> {
        let email = email_repo::get(&self.pool, email_id).await?;

        if !matches!(email.status, EmailStatus::Pending | EmailStatus::Processing) {
            info!(status = %email.status.as_str(), "skipping non-pending email");
            return Ok(());
        }

        email_repo::set_status(&self.pool, email_id, EmailStatus::Processing, None).await?;

        let thread = thread_repo::get(&self.pool, email.thread_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("thread {}", email.thread_id)))?;

        let run = run_repo::start(&self.pool, email_id, email.thread_id, RunTrigger::Poll).await?;

        let corr = Correlation::new(email_id, run.id);
        lifecycle::processing_started(&corr);
        let started = std::time::Instant::now();

        let result = self.run_pipeline(&corr, &run.id, &email_id, &thread).await;
        let duration_ms = started.elapsed().as_millis() as u64;

        match &result {
            Ok(status) => {
                info!(status = %status.as_str(), "pipeline completed");
                email_repo::set_status(&self.pool, email_id, *status, None).await?;
                run_repo::finish(&self.pool, run.id, RunState::Succeeded, None).await?;
                lifecycle::processing_completed(&corr, status.as_str(), duration_ms);
            }
            Err(e) if e.is_retryable() => {
                warn!(error = %e, "retryable error");
                let attempts =
                    email_repo::record_error(&self.pool, email_id, &e.to_string()).await?;
                lifecycle::processing_failed(
                    &corr,
                    errors::app_error_type(e),
                    &e.to_string(),
                    attempts,
                );
                run_repo::finish(&self.pool, run.id, RunState::Failed, Some(&e.to_string()))
                    .await?;
            }
            Err(e) => {
                // A malformed agent output is a model glitch, not a broken
                // message: flag it for human review instead of killing it.
                let needs_review = matches!(&e, AppError::Agent(_));
                error!(error = %e, needs_review, "pipeline failed");
                let attempts =
                    email_repo::record_error(&self.pool, email_id, &e.to_string()).await?;
                lifecycle::processing_failed(
                    &corr,
                    errors::app_error_type(e),
                    &e.to_string(),
                    attempts,
                );
                if needs_review {
                    email_repo::set_status(&self.pool, email_id, EmailStatus::NeedsReview, None)
                        .await?;
                } else {
                    email_repo::set_status(&self.pool, email_id, EmailStatus::Dead, None).await?;
                }
                run_repo::finish(&self.pool, run.id, RunState::Failed, Some(&e.to_string()))
                    .await?;
            }
        }

        result.map(|_| ())
    }
}
