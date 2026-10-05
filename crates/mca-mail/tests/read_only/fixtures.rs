//! Fixtures shared by the read-only contract tests.

use mca_mail::domain::{EmailAddress, InboundMessage, OutboundMessage};
use mca_mail::error::MailError;
use mca_mail::mail::MockMailProvider;

/// A probe message whose subject carries a fresh UUID, so a long-lived test
/// database never collides with a row an earlier run left behind.
pub fn message(subject: &str, from: &str, body: &str) -> InboundMessage {
    let nonce = uuid::Uuid::new_v4();
    InboundMessage {
        provider_message_id: format!("pmid-{nonce}"),
        internet_message_id: Some(format!("<{nonce}@example.com>")),
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::new(from),
        to: vec![EmailAddress::new("inbox@mca.example")],
        cc: vec![],
        subject: format!("{subject} {nonce}"),
        date: None,
        text_body: body.into(),
        attachments: vec![],
        total_size: body.len(),
    }
}

/// Whether a result is the typed read-only refusal, and nothing else.
pub fn is_refusal<T>(result: Result<T, MailError>) -> bool {
    matches!(result, Err(MailError::OperationNotAllowed { .. }))
}

pub fn outbound() -> OutboundMessage {
    OutboundMessage {
        to: vec![EmailAddress::new("buyer@example.com")],
        cc: vec![],
        subject: "Re: pricing".into(),
        text_body: "Here is our pricing.".into(),
        html_body: None,
        in_reply_to: None,
        references: vec![],
        attachments: vec![],
    }
}

/// A read-only mock seeded with one message, ready to be read.
pub fn seeded_read_only() -> MockMailProvider {
    MockMailProvider::with_messages(vec![message("Unseen probe", "sender@example.com", "body")])
}
