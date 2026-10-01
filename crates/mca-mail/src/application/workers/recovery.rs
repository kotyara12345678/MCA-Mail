//! Restart recovery: fail orphaned runs and re-queue interrupted emails.

use std::time::Instant;

use sqlx::PgPool;
use tracing::info;

use crate::error::AppError;
use crate::observability::system;

/// On a single-instance start, any run still marked `running` is orphaned by
/// the previous process. Fail them first, then reset their emails to
/// `pending` so the pipeline resumes from the recorded stage.
pub async fn recover_stuck_emails(pool: &PgPool) -> Result<u64, AppError> {
    system::recovery_started();
    let started = Instant::now();

    let runs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM email_processing_runs WHERE state = 'running'")
            .fetch_one(pool)
            .await?;
    let stuck_ids: Vec<uuid::Uuid> =
        sqlx::query_scalar("SELECT id FROM emails WHERE status = 'processing'")
            .fetch_all(pool)
            .await?;
    let emails = stuck_ids.len() as i64;

    if runs > 0 || emails > 0 {
        system::stuck_emails_found(runs, emails);
        for id in &stuck_ids {
            system::processing_recovered(*id);
        }

        sqlx::query(
            "UPDATE email_processing_runs SET state = 'failed', finished_at = now() \
             WHERE state = 'running'",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "UPDATE emails SET status = 'pending', last_error = 'interrupted by restart' \
             WHERE status = 'processing'",
        )
        .execute(pool)
        .await?;
        info!(orphan_runs = runs, emails, "recovered interrupted emails");
    }

    system::recovery_completed(runs, emails, started.elapsed().as_millis() as u64);
    Ok(emails as u64)
}
