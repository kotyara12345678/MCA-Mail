use sqlx::PgPool;

use crate::config::RetentionSettings;
use crate::error::AppError;

/// What one retention pass removed, so the operator can see the effect in the
/// metrics and in the log without inspecting rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct RetentionReport {
    pub emails_anonymized: u64,
    pub attachment_extracts_purged: u64,
    pub events_purged: u64,
    pub audit_rows_purged: u64,
}

impl RetentionReport {
    pub fn total(&self) -> u64 {
        self.emails_anonymized
            + self.attachment_extracts_purged
            + self.events_purged
            + self.audit_rows_purged
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// Run one retention pass.
///
/// Bodies are anonymized rather than deleted by default, which keeps the
/// operational history of a lead coherent while still ageing out personal data
/// according to the approved policy.
pub async fn run(pool: &PgPool, settings: &RetentionSettings) -> Result<RetentionReport, AppError> {
    if !settings.enabled {
        return Ok(RetentionReport::default());
    }
    let report = RetentionReport {
        emails_anonymized: super::email_repo::anonymize(pool, settings.email_days).await?,
        attachment_extracts_purged: super::email_repo::purge_attachment_text(
            pool,
            settings.attachment_days,
        )
        .await?,
        events_purged: super::event_repo::purge_events(pool, settings.event_days).await?,
        audit_rows_purged: super::audit_repo::purge(pool, settings.audit_days).await?,
    };
    Ok(report)
}
