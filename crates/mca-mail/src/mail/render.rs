//! Outbound message → RFC5322.
//!
//! Shared by SMTP delivery and IMAP `APPEND`, so a draft saved in the mailbox
//! is byte-identical to what would have been sent.

use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::Message;

use crate::domain::{EmailAddress, OutboundMessage};
use crate::error::MailError;

/// Largest outbound message accepted, in bytes.
///
/// Checked after rendering: MIME and base64 encoding inflate the original
/// attachment size by up to a third, so a pre-render check would under-count.
pub const MAX_OUTBOUND_BYTES: usize = 25 * 1024 * 1024;

pub fn build(
    message: &OutboundMessage,
    from_address: &str,
    from_name: &str,
) -> Result<Message, MailError> {
    if message.to.is_empty() {
        return Err(MailError::Rejected("no recipients".into()));
    }
    let mut builder = Message::builder()
        .from(mailbox(from_address, Some(from_name.to_string()))?)
        .subject(message.subject.clone());

    for to in &message.to {
        builder = builder.to(mailbox(&to.address, to.name.clone())?);
    }
    for cc in &message.cc {
        builder = builder.cc(mailbox(&cc.address, cc.name.clone())?);
    }
    // Without these a reply still reaches the customer but no longer belongs to
    // the thread the lead model tracks, so later turns go unmatched.
    if let Some(parent) = message.in_reply_to.as_deref().filter(|v| !v.is_empty()) {
        builder = builder.in_reply_to(parent.to_string());
    }
    if !message.references.is_empty() {
        // RFC5322 wants one folded line; the vector is joined rather than
        // repeated because repeated `References` headers are not interoperable.
        builder = builder.references(message.references.join(" "));
    }

    let built = match message.html_body.as_deref() {
        // Plain part first: clients that honour `alternative` show it, and a
        // customer reading only text is the common case.
        Some(html) => builder.multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(message.text_body.clone()))
                .singlepart(SinglePart::html(html.to_string())),
        ),
        None => builder.body(message.text_body.clone()),
    };
    built.map_err(|e| MailError::Rejected(e.to_string()))
}

/// Render to bytes for IMAP `APPEND`, with a size guard.
pub fn rfc5322(message: &OutboundMessage, from_address: &str) -> Result<Vec<u8>, MailError> {
    let rendered = build(message, from_address, "MCA Logistics")?;
    let bytes = rendered.formatted();
    if bytes.len() > MAX_OUTBOUND_BYTES {
        return Err(MailError::TooLarge(format!(
            "outbound message is {} bytes, limit is {MAX_OUTBOUND_BYTES}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

fn mailbox(address: &str, name: Option<String>) -> Result<Mailbox, MailError> {
    let parsed = address
        .trim()
        .parse::<lettre::Address>()
        .map_err(|_| MailError::Rejected(format!("invalid email address: {address}")))?;
    Ok(Mailbox::new(name.filter(|n| !n.trim().is_empty()), parsed))
}

/// Every address that will receive this message, in `to`-then-`cc` order.
#[allow(dead_code, reason = "used by the outbox worker once it is wired up")]
pub fn recipients(message: &OutboundMessage) -> Vec<EmailAddress> {
    message
        .to
        .iter()
        .chain(message.cc.iter())
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "render_test.rs"]
mod tests;
