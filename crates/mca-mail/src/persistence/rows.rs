use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

/// Row shape of `emails`. Enum columns arrive as `TEXT` and are converted by
/// the repository, so an unexpected legacy value surfaces as a decode error
/// instead of silently defaulting.
#[derive(Debug, Clone, FromRow)]
pub struct EmailRow {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub direction: String,
    pub status: String,
    pub category: Option<String>,
    pub spam_verdict: Option<String>,
    pub mailbox_action: String,
    pub provider: String,
    pub mailbox: String,
    pub provider_uid: Option<String>,
    pub provider_uid_validity: Option<i64>,
    pub internet_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub from_address: String,
    pub from_name: Option<String>,
    pub to_addresses: Vec<String>,
    pub cc_addresses: Vec<String>,
    pub subject: String,
    pub normalized_subject: String,
    pub date: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    pub text_body: String,
    pub html_present: bool,
    pub size_bytes: i64,
    pub lead_id: Option<Uuid>,
    pub labels: Vec<String>,
    pub dedup_key: String,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub processed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Columns always selected for a full email read.
pub const EMAIL_COLUMNS: &str = "id, thread_id, direction, status, category, spam_verdict, \
     mailbox_action, provider, mailbox, provider_uid, provider_uid_validity, internet_message_id, \
     in_reply_to, \"references\", from_address, from_name, to_addresses, cc_addresses, subject, \
     normalized_subject, date, received_at, text_body, html_present, size_bytes, lead_id, labels, \
     dedup_key, attempts, last_error, processed_at, created_at";

/// Compact projection used by list endpoints; excludes the body.
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct EmailSummaryRow {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub status: String,
    pub category: Option<String>,
    pub spam_verdict: Option<String>,
    pub from_address: String,
    pub from_name: Option<String>,
    pub subject: String,
    received_at: DateTime<Utc>,
    pub lead_id: Option<Uuid>,
    pub size_bytes: i64,
    pub attempts: i32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ThreadRow {
    pub id: Uuid,
    pub root_message_id: Option<String>,
    pub normalized_subject: String,
    pub conversation_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct AttachmentRow {
    pub id: Uuid,
    pub email_id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub content_id: Option<String>,
    pub is_inline: bool,
    pub sha256: String,
    pub extraction: String,
    pub extracted_chars: i32,
    pub text_excerpt: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Insert payload for a new inbound message.
#[derive(Debug, Clone)]
pub struct NewEmail<'a> {
    pub thread_id: Uuid,
    pub provider: &'a str,
    pub mailbox: &'a str,
    pub provider_uid: Option<&'a str>,
    pub provider_uid_validity: Option<i64>,
    pub internet_message_id: Option<&'a str>,
    pub in_reply_to: Option<&'a str>,
    pub references: &'a [String],
    pub from_address: &'a str,
    pub from_name: Option<&'a str>,
    pub to_addresses: &'a [String],
    pub cc_addresses: &'a [String],
    pub subject: &'a str,
    pub normalized_subject: &'a str,
    pub date: Option<DateTime<Utc>>,
    pub text_body: &'a str,
    pub html_present: bool,
    pub size_bytes: i64,
    pub dedup_key: &'a str,
}

#[derive(Debug, Clone, FromRow)]
pub struct AttachmentInput {
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub content_id: Option<String>,
    pub is_inline: bool,
    pub sha256: String,
    pub extraction: String,
    pub extracted_chars: i32,
    pub text_excerpt: Option<String>,
}
