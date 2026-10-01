use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ids::ThreadId;
use super::message::{OutboundAttachment, OutboundMessage};

/// Channel-neutral inbound envelope.
///
/// Telegram, WhatsApp Business API, a website form and SIP telephony will all
/// adapt into this type, which lets the classification, qualification,
/// logistics and handoff agents stay channel-agnostic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboundEnvelope {
    pub channel: String,
    pub external_message_id: String,
    pub external_thread_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub from: ContactRef,
    pub subject: String,
    pub text: String,
    pub attachments: Vec<OutboundAttachment>,
    pub received_at: DateTime<Utc>,
    pub metadata: ChannelMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContactRef {
    pub external_id: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelMetadata {
    /// Channel-specific identifiers kept opaque to the business logic.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboundEnvelope {
    pub channel: String,
    pub in_reply_to: Option<String>,
    pub to: ContactRef,
    pub text: String,
    pub metadata: ChannelMetadata,
}

impl OutboundEnvelope {
    pub fn new(to: ContactRef, text: impl Into<String>) -> Self {
        Self {
            channel: "email".to_string(),
            in_reply_to: None,
            to,
            text: text.into(),
            metadata: ChannelMetadata::default(),
        }
    }
}

/// Persisted, channel-neutral conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ThreadId,
    pub channel: String,
    pub external_thread_id: Option<String>,
    pub contact_key: String,
    pub subject: String,
    pub last_message_at: DateTime<Utc>,
}

/// A transport for one communication channel.
///
/// Implemented today by the e-mail gateway. Additional channels are adapters
/// behind this same trait, so the agents and business core need no changes.
#[async_trait]
pub trait CommunicationChannel: Send + Sync {
    fn name(&self) -> &'static str;

    /// Poll the channel for new inbound messages.
    async fn fetch_new(&self) -> Result<Vec<InboundEnvelope>, ChannelError>;

    /// Deliver a message. Implementations must be idempotent on
    /// `OutboundEnvelope::metadata["idempotency_key"]`.
    async fn send(&self, message: OutboundEnvelope) -> Result<String, ChannelError>;

    /// Whether the channel is currently usable, for `/ready`.
    async fn health(&self) -> ChannelHealth {
        ChannelHealth::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelHealth {
    pub connected: bool,
    pub detail: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("channel transport error: {0}")]
    Transport(String),
    #[error("channel authentication failed: {0}")]
    Auth(String),
    #[error("channel temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("channel rejected the message: {0}")]
    Rejected(String),
}

impl From<OutboundMessage> for OutboundEnvelope {
    fn from(value: OutboundMessage) -> Self {
        let to = value
            .first_recipient()
            .map(|a| ContactRef {
                external_id: a.address.clone(),
                display_name: a.name.clone(),
                username: None,
                phone: None,
                email: Some(a.address.clone()),
            })
            .unwrap_or(ContactRef {
                external_id: String::new(),
                display_name: None,
                username: None,
                phone: None,
                email: None,
            });
        let mut envelope = OutboundEnvelope::new(to, value.text_body);
        envelope.in_reply_to = value.in_reply_to.clone();
        envelope
    }
}

impl From<crate::error::MailError> for ChannelError {
    /// Preserve the transport's failure class so a caller can tell "retry later"
    /// from "this will never be accepted" without parsing strings.
    fn from(value: crate::error::MailError) -> Self {
        use crate::error::MailError;
        match value {
            MailError::Auth(detail) => ChannelError::Auth(detail),
            MailError::Connect(detail) | MailError::Unavailable(detail) => {
                ChannelError::Unavailable(detail)
            }
            MailError::Rejected(detail) | MailError::BlockedAttachment(detail) => {
                ChannelError::Rejected(detail)
            }
            MailError::TooLarge(detail) => ChannelError::Rejected(detail),
            other => ChannelError::Transport(other.to_string()),
        }
    }
}
