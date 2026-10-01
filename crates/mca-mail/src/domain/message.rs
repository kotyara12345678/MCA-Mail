use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::EmailAddress;

/// A MIME part of an inbound message, before it is split into body and
/// attachments by the mail gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessagePart {
    /// `text/plain` body, already HTML-stripped and whitespace-normalized.
    PlainText(String),
    /// `text/html` body converted to safe plain text.
    HtmlText(String),
    /// Binary or unhandled attachment payload. Held in memory only for the
    /// duration of parsing; never persisted raw.
    Attachment {
        filename: String,
        mime_type: String,
        size: usize,
        content_id: Option<String>,
        data: Vec<u8>,
    },
    /// Any other part: kept as an opaque descriptor so the audit trail can
    /// explain what was present without retaining the payload.
    Opaque { mime_type: String, size: usize },
}

/// Outcome of running an attachment through the text-extraction pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ExtractionStatus {
    /// Text was extracted and is safe to feed to an agent.
    Extracted { characters: usize },
    /// The format has no registered extractor; the agent is told explicitly.
    UnsupportedFormat,
    /// The attachment was rejected before reading (size, blocked extension).
    Rejected { reason: String },
    /// An extractor ran and failed; recorded, never silently ignored.
    Failed { reason: String },
    /// Extraction was not attempted because the payload is not text-like.
    NotAttempted,
}

impl ExtractionStatus {
    pub fn has_text(&self) -> bool {
        matches!(self, ExtractionStatus::Extracted { .. })
    }
}

/// A stored attachment record. Raw bytes are intentionally not retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttachment {
    pub filename: String,
    pub mime_type: String,
    pub size: usize,
    pub content_id: Option<String>,
    pub is_inline: bool,
    pub sha256: String,
    pub extraction: ExtractionStatus,
    /// Bounded plain-text extract, only present when `extraction` succeeded.
    pub text_excerpt: Option<String>,
}

impl EmailAttachment {
    pub fn new(
        filename: impl Into<String>,
        mime_type: impl Into<String>,
        size: usize,
        sha256: impl Into<String>,
    ) -> Self {
        Self {
            filename: filename.into(),
            mime_type: mime_type.into(),
            size,
            content_id: None,
            is_inline: false,
            sha256: sha256.into(),
            extraction: ExtractionStatus::NotAttempted,
            text_excerpt: None,
        }
    }
}

/// A fully parsed inbound message as delivered by the mail gateway.
///
/// This type is the boundary between untrusted external input and the rest of
/// the system. Nothing downstream is allowed to read raw MIME; agents only ever
/// see `text_body` plus bounded attachment extracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundMessage {
    pub provider_message_id: String,
    pub internet_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub from: EmailAddress,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub subject: String,
    pub date: Option<DateTime<Utc>>,
    pub text_body: String,
    pub attachments: Vec<EmailAttachment>,
    /// Total decoded size in bytes, used for size guards and metrics.
    pub total_size: usize,
}

impl InboundMessage {
    /// True when the message looks like an automatic system notification.
    pub fn has_automated_headers(&self) -> bool {
        self.internet_message_id.as_deref().is_some_and(|id| {
            let lower = id.to_ascii_lowercase();
            lower.contains("noreply")
                || lower.contains("no-reply")
                || lower.contains("notification")
        })
    }

    /// All addresses that could identify the counterparty of this thread.
    pub fn participants(&self) -> Vec<&EmailAddress> {
        let mut out = vec![&self.from];
        out.extend(self.to.iter());
        out.extend(self.cc.iter());
        out
    }
}

/// A message the system wants to deliver through the mail gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub subject: String,
    pub text_body: String,
    pub html_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<OutboundAttachment>,
}

/// An attachment the system is explicitly allowed to attach to an outbound mail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundAttachment {
    pub filename: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

impl OutboundMessage {
    pub fn plain(to: EmailAddress, subject: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            to: vec![to],
            cc: Vec::new(),
            subject: subject.into(),
            text_body: body.into(),
            html_body: None,
            in_reply_to: None,
            references: Vec::new(),
            attachments: Vec::new(),
        }
    }

    pub fn first_recipient(&self) -> Option<&EmailAddress> {
        self.to.first()
    }
}

/// Identity of the mailbox the service operates on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxIdentity {
    pub address: String,
    pub display_name: String,
}

impl Default for MailboxIdentity {
    fn default() -> Self {
        Self {
            address: "mca@example.invalid".to_string(),
            display_name: "MCA Logistics".to_string(),
        }
    }
}
