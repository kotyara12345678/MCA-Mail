//! Mail I/O. The domain and the orchestrator never see a socket.

pub mod folders;
mod guard;
pub(crate) mod idle;
mod imap;
mod mock;
mod ops;
mod parse;
mod policy;
#[cfg(test)]
#[path = "read_only_tests.rs"]
mod read_only_tests;
mod render;
mod smtp;
mod tls;
mod uid;
mod writer;

pub use guard::MailGuard;
pub use imap::ImapMailProvider;
pub use mock::{MailboxMutationLog, MockMailProvider};
pub use ops::MailboxOp;
pub use policy::{OutboundDecision, OutboundPolicyGuard, PolicyContext, PolicyDenial};
pub use render::MAX_OUTBOUND_BYTES;
pub use smtp::{SmtpTransport, TransportAuth};
pub use uid::{FetchBatch, UidState};
pub use writer::MailboxWriter;

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
    ///
    /// Kept for the channel-neutral view and tooling; the poll cycle uses
    /// [`MailProvider::fetch_after`], which is bounded by the persisted
    /// high-water mark rather than by server-side flags.
    async fn fetch_new(&self) -> Result<Vec<InboundMessage>, MailError>;

    /// Server-side UID state of the selected mailbox, read fresh each cycle.
    ///
    /// The poll worker compares it against the stored cursor to detect a
    /// first run (record a boundary, fetch nothing) and a UIDVALIDITY change
    /// (re-bound without backfill).
    async fn uid_state(&self) -> Result<UidState, MailError>;

    /// Messages with UID strictly greater than `after_uid`, oldest first, at
    /// most one configured batch.
    ///
    /// Implementations must sort the search result before applying the batch
    /// limit: `UID SEARCH` returns a `HashSet` with an arbitrary order, and an
    /// unordered `take(limit)` would pick a random slice of history instead of
    /// the oldest new mail. `highest_uid` is the highest UID examined,
    /// including bodies skipped during parsing — the caller advances the
    /// watermark to it so a poison message cannot stall the range behind it.
    async fn fetch_after(&self, after_uid: u32) -> Result<FetchBatch, MailError>;

    /// Whether the transport is usable. Must never perform a real send.
    ///
    /// There is deliberately no `send` here. Reads live on this trait and
    /// mutations live on [`MailboxWriter`], so a `&dyn MailProvider` — which is
    /// what the read-only pipeline and the health check hold — cannot reach SMTP
    /// even by accident.
    async fn health(&self) -> ChannelHealth;
}

/// A provider that can be asked to write, with the read-only guard applied.
///
/// Callers that need mutations take this trait; callers that only read take
/// [`MailProvider`]. A `&dyn MailProvider` therefore has no route to SMTP.
pub trait MaybeWritable: MailProvider + MailboxWriter {}

impl<T: MailProvider + MailboxWriter> MaybeWritable for T {}

/// Channel-neutral view, so a conversation component does not depend on email.
///
/// Bounded on [`MaybeWritable`] rather than [`MailProvider`], because
/// `CommunicationChannel` has a `send` method: the send goes through
/// `MailboxWriter`, and therefore through the read-only guard, on its way out.
pub struct EmailChannel<C: MaybeWritable> {
    inner: C,
}

impl<C: MaybeWritable> EmailChannel<C> {
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
impl<C: MaybeWritable> CommunicationChannel for EmailChannel<C> {
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
        // The guard runs inside `MailboxWriter::send`, so a read-only deployment
        // gets `OperationNotAllowed` here rather than a connection to a mailbox.
        let message = from_outbound_envelope(&envelope);
        MailboxWriter::send(&self.inner, &message)
            .await
            .map_err(Into::into)
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
///
/// The mock inherits `settings.mode` rather than always defaulting, so a
/// read-only deployment exercises the same guard on the mock as in production.
pub fn build(settings: &MailSettings) -> Result<Box<dyn MailProvider>, MailError> {
    Ok(construct(settings)?)
}

/// Build a writer-capable handle, for components that legitimately send.
///
/// Callers that only read should take [`MailProvider`] instead; that is what
/// keeps the read-only pipeline structurally unable to mutate the mailbox.
pub fn build_writable(settings: &MailSettings) -> Result<Box<dyn MaybeWritable>, MailError> {
    construct(settings)
}

fn construct(settings: &MailSettings) -> Result<Box<dyn MaybeWritable>, MailError> {
    match settings.provider {
        MailProviderKind::Mock => Ok(Box::new(MockMailProvider::from_corpus_dir(
            settings.mock_corpus_dir.clone(),
            settings.mode,
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
