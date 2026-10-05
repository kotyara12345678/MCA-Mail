use sqlx::PgPool;

use crate::backup::BackupEvent;
use crate::persistence::event_repo::{self, NewEvent};

use super::{EVENT_BACKUP_COMPLETED, EVENT_BACKUP_FAILED};

/// Persist one backup outcome without recording the database URL or credentials.
pub(crate) async fn record(pool: &PgPool, event: BackupEvent) -> anyhow::Result<()> {
    let (code, severity, message, details) = match event {
        BackupEvent::Completed(outcome) => (
            EVENT_BACKUP_COMPLETED,
            crate::domain::EventSeverity::Info,
            format!("backup written: {}", outcome.file_name),
            serde_json::json!({
                "file_name": outcome.file_name,
                "size_bytes": outcome.size_bytes,
                "removed": outcome.removed.len(),
            }),
        ),
        BackupEvent::Failed(_) => (
            EVENT_BACKUP_FAILED,
            crate::domain::EventSeverity::Error,
            "scheduled backup failed".to_string(),
            serde_json::json!({ "error": "backup operation failed" }),
        ),
    };
    let event = NewEvent {
        run_id: None,
        email_id: None,
        lead_id: None,
        stage: None,
        severity,
        code: code.to_string(),
        message,
        details,
    };
    event_repo::event(pool, &event).await?;
    Ok(())
}
