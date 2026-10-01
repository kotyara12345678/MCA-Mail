use sqlx::PgPool;

use super::super::rows::AttachmentRow;
use super::update::truncate;
use crate::domain::{EmailAttachment, EmailId, ExtractionStatus};
use crate::error::AppError;

fn extraction_label(status: &ExtractionStatus) -> String {
    match status {
        ExtractionStatus::Extracted { characters } => format!("extracted:{characters}"),
        ExtractionStatus::UnsupportedFormat => "unsupported_format".to_string(),
        ExtractionStatus::Rejected { .. } => "rejected".to_string(),
        ExtractionStatus::Failed { .. } => "failed".to_string(),
        ExtractionStatus::NotAttempted => "not_attempted".to_string(),
    }
}

fn extracted_chars(status: &ExtractionStatus) -> i32 {
    match status {
        ExtractionStatus::Extracted { characters } => *characters as i32,
        _ => 0,
    }
}

/// Store attachment metadata and the bounded text extract.
///
/// Raw attachment bytes are never written: the corpus copy lives in the
/// corporate mailbox, and the extract is capped so an oversized document
/// cannot be smuggled into an LLM prompt.
pub async fn insert_attachments(
    pool: &PgPool,
    email_id: EmailId,
    attachments: &[EmailAttachment],
) -> Result<Vec<uuid::Uuid>, AppError> {
    let mut ids = Vec::with_capacity(attachments.len());
    for a in attachments {
        let id = sqlx::query_scalar::<_, uuid::Uuid>(
            "INSERT INTO email_attachments (email_id, filename, mime_type, size_bytes, \
             content_id, is_inline, sha256, extraction, extracted_chars, text_excerpt) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id",
        )
        .bind(email_id)
        .bind(truncate(&a.filename, 512))
        .bind(&a.mime_type)
        .bind(a.size as i64)
        .bind(&a.content_id)
        .bind(a.is_inline)
        .bind(&a.sha256)
        .bind(extraction_label(&a.extraction))
        .bind(extracted_chars(&a.extraction))
        .bind(a.text_excerpt.as_deref().map(|t| truncate(t, 8000)))
        .fetch_one(pool)
        .await?;
        ids.push(id);
    }
    Ok(ids)
}

pub async fn attachments(pool: &PgPool, email_id: EmailId) -> Result<Vec<AttachmentRow>, AppError> {
    let rows = sqlx::query_as::<_, AttachmentRow>(
        "SELECT id, email_id, filename, mime_type, size_bytes, content_id, is_inline, sha256, \
         extraction, extracted_chars, text_excerpt, created_at FROM email_attachments \
         WHERE email_id = $1 ORDER BY created_at",
    )
    .bind(email_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Drop extracts of old attachments while keeping the metadata.
pub async fn purge_attachment_text(pool: &PgPool, older_than_days: i32) -> Result<u64, AppError> {
    let done = sqlx::query(
        "UPDATE email_attachments SET text_excerpt = NULL, extraction = 'purged' \
         WHERE text_excerpt IS NOT NULL \
         AND created_at < now() - make_interval(days => $1)",
    )
    .bind(older_than_days)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}
