use super::{MailProvider, MailboxWriter, MockMailProvider};
use crate::config::MailMode;
use crate::domain::{EmailAddress, InboundMessage, OutboundMessage};
use crate::error::MailError;

#[path = "read_only_mutation_tests.rs"]
mod mutations;
#[path = "read_only_policy_tests.rs"]
mod policy;
#[path = "read_only_read_tests.rs"]
mod reads;

fn inbound() -> InboundMessage {
    InboundMessage {
        provider_message_id: "pmid-1".into(),
        internet_message_id: Some("<a@example.com>".into()),
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::new("customer@example.com"),
        to: vec![EmailAddress::new("inbox@mca.example")],
        cc: vec![],
        subject: "Need a quote".into(),
        date: None,
        text_body: "please quote 2 pallets".into(),
        attachments: vec![],
        total_size: 20,
    }
}

fn outbound() -> OutboundMessage {
    OutboundMessage::plain(
        EmailAddress::new("customer@example.com"),
        "Re: quote",
        "here",
    )
}

fn mock(mode: MailMode) -> MockMailProvider {
    MockMailProvider::with_mode(mode)
}

fn is_refusal(error: &MailError) -> bool {
    matches!(error, MailError::OperationNotAllowed { .. })
}
