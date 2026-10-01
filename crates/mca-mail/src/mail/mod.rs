//! Mail I/O. The domain and the orchestrator never see a socket.

mod imap;
mod mock;
mod parse;
mod policy;
mod render;
mod smtp;
mod tls;

pub use imap::ImapMailProvider;
pub use mock::MockMailProvider;
pub use policy::{OutboundDecision, OutboundPolicyGuard, PolicyContext, PolicyDenial};
pub use render::MAX_OUTBOUND_BYTES;
pub use smtp::{SmtpTransport, TransportAuth};

use async_trait::async_trait;

use crate::config::{MailProviderKind, MailSettings};
use crate::domain::{
    ChannelHealth, CommunicationChannel, InboundEnvelope, InboundMessage, OutboundMessage,
};
use crate::error::MailError;

/// Transport for one mailbox: inbound fetch, outbound send, liveness.
///
/// Implementations must be idempotent on the outbound message's idempotency
/// key, so a worker restart between "sent" and "committed" cannot duplicate a
/// reply to a customer.
#[async_trait]
pub trait MailProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Optional one-time setup (e.g. load a mock corpus from disk). Default is
    /// a no-op so real transports do not have to implement it.
    async fn init(&self) -> Result<(), MailError> {
        Ok(())
    }

    /// Fetch messages not yet handed to the pipeline.
    async fn fetch_new(&self) -> Result<Vec<InboundMessage>, MailError>;

    /// Deliver a message, returning the provider's message identifier.
    async fn send(&self, message: &OutboundMessage) -> Result<String, MailError>;

    /// Whether the transport is usable. Must never perform a real send.
    async fn health(&self) -> ChannelHealth;
}

/// Channel-neutral view, so a conversation component does not depend on email.
pub struct EmailChannel<C: MailProvider> {
    inner: C,
}

impl<C: MailProvider> EmailChannel<C> {
    pub fn new(inner: C) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &C {
        &self.inner
    }

    pub fn into_inner(self) -> C {
        self.inner
    }
}

#[async_trait]
impl<C: MailProvider> CommunicationChannel for EmailChannel<C> {
    fn name(&self) -> &'static str {
        "email"
    }

    async fn fetch_new(&self) -> Result<Vec<InboundEnvelope>, crate::domain::ChannelError> {
        self.inner
            .fetch_new()
            .await
            .map(|messages| messages.into_iter().map(to_inbound_envelope).collect())
            .map_err(Into::into)
    }

    async fn send(
        &self,
        envelope: crate::domain::OutboundEnvelope,
    ) -> Result<String, crate::domain::ChannelError> {
        let message = from_outbound_envelope(&envelope);
        self.inner.send(&message).await.map_err(Into::into)
    }

    async fn health(&self) -> ChannelHealth {
        self.inner.health().await
    }
}

/// Project an inbound message onto the channel-neutral envelope.
///
/// The whole `to`/`cc` lists travel in `extra` rather than in dedicated fields
/// because `ContactRef` models a single counterparty; losing recipients here
/// would be worse than carrying them opaquely.
pub fn to_inbound_envelope(message: InboundMessage) -> InboundEnvelope {
    let references = message.references.clone();
    let to_list: Vec<String> = message.to.iter().map(|a| a.display()).collect();
    let cc_list: Vec<String> = message.cc.iter().map(|a| a.display()).collect();
    let attachments: Vec<crate::domain::OutboundAttachment> = message
        .attachments
        .iter()
        .map(|a| crate::domain::OutboundAttachment {
            filename: a.filename.clone(),
            mime_type: a.mime_type.clone(),
            // Only the bounded text extract travels; raw bytes never enter a
            // channel-neutral envelope.
            data: a.text_excerpt.clone().unwrap_or_default().into_bytes(),
        })
        .collect();
    let mut extra = serde_json::Map::new();
    extra.insert("to".into(), serde_json::json!(to_list));
    extra.insert("cc".into(), serde_json::json!(cc_list));
    extra.insert("references".into(), serde_json::json!(references));
    extra.insert("subject".into(), serde_json::json!(message.subject));
    extra.insert("size".into(), serde_json::json!(message.total_size));
    InboundEnvelope {
        channel: "email".to_string(),
        external_message_id: message.provider_message_id,
        external_thread_id: message.internet_message_id,
        in_reply_to: message.in_reply_to,
        from: crate::domain::ContactRef {
            external_id: message.from.address.clone(),
            display_name: message.from.name.clone(),
            username: None,
            phone: None,
            email: Some(message.from.address),
        },
        subject: message.subject,
        text: message.text_body,
        attachments,
        received_at: chrono::Utc::now(),
        metadata: crate::domain::ChannelMetadata { extra },
    }
}

/// Project a channel-neutral envelope back onto an outbound email.
///
/// Lossy by design: a channel that carries no subject or sender cannot be
/// represented as email. Defaults come from metadata, and an envelope without a
/// recipient is a programming error rather than a sendable message.
pub fn from_outbound_envelope(envelope: &crate::domain::OutboundEnvelope) -> OutboundMessage {
    let recipient = envelope
        .to
        .email
        .clone()
        .unwrap_or_else(|| envelope.to.external_id.clone());
    let extra = &envelope.metadata.extra;
    let str_list = |key: &str| -> Vec<String> {
        extra
            .get(key)
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let text = |key: &str| -> Option<String> {
        extra.get(key).and_then(|v| v.as_str()).map(str::to_string)
    };
    let mut to = vec![recipient];
    // The primary recipient is already in `to`; metadata usually repeats it,
    // and sending twice would put two copies in the customer's inbox.
    for extra in str_list("to") {
        if !to
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&extra))
        {
            to.push(extra);
        }
    }
    let mut cc: Vec<String> = str_list("cc")
        .into_iter()
        .filter(|c| !to.iter().any(|existing| existing.eq_ignore_ascii_case(c)))
        .collect();
    cc.dedup();
    OutboundMessage {
        to: to
            .into_iter()
            .map(crate::domain::EmailAddress::new)
            .collect(),
        cc: cc
            .into_iter()
            .map(crate::domain::EmailAddress::new)
            .collect(),
        subject: text("subject").unwrap_or_default(),
        text_body: envelope.text.clone(),
        html_body: text("html_body"),
        in_reply_to: envelope.in_reply_to.clone(),
        references: str_list("references"),
        attachments: vec![],
    }
}

/// Build the configured provider.
///
/// Mock is the default so a fresh checkout runs end to end without a mailbox.
/// A real transport is only constructed when explicitly selected, which keeps
/// an accidental production boot from reaching a live server.
pub fn build(settings: &MailSettings) -> Result<Box<dyn MailProvider>, MailError> {
    match settings.provider {
        MailProviderKind::Mock => Ok(Box::new(MockMailProvider::from_corpus_dir(
            settings.mock_corpus_dir.clone(),
        ))),
        MailProviderKind::Imap => {
            if settings.username.trim().is_empty() {
                return Err(MailError::Auth("MAIL_IMAP_USER is empty".into()));
            }
            let smtp = SmtpTransport::new(
                settings,
                TransportAuth::new(settings.username.clone(), settings.password.expose_owned()),
            )?;
            Ok(Box::new(ImapMailProvider::new(settings, smtp)))
        }
    }
}
