//! Tests for `render.rs`.

#![cfg(test)]

use super::*;
use super::*;

fn message() -> OutboundMessage {
    OutboundMessage {
        to: vec![EmailAddress::new("Customer@Example.com")],
        cc: vec![EmailAddress::new("boss@example.com")],
        subject: "Re: Quote".into(),
        text_body: "Body text".into(),
        html_body: None,
        in_reply_to: Some("<parent@example.com>".into()),
        references: vec!["<root@example.com>".into(), "<parent@example.com>".into()],
        attachments: vec![],
    }
}

#[test]
fn threading_headers_survive_rendering() {
    let raw = String::from_utf8(rfc5322(&message(), "agent@mca.example", "MCA").unwrap()).unwrap();
    assert!(raw.contains("In-Reply-To: <parent@example.com>"), "{raw}");
    assert!(
        raw.contains("References: <root@example.com> <parent@example.com>"),
        "{raw}"
    );
}

#[test]
fn invalid_recipient_is_refused_before_sending() {
    let mut msg = message();
    msg.to = vec![EmailAddress::new("not-an-address")];
    assert!(matches!(
        build(&msg, "agent@mca.example", "MCA"),
        Err(MailError::Rejected(_))
    ));
}

#[test]
fn empty_recipient_list_is_refused() {
    let mut msg = message();
    msg.to.clear();
    assert!(build(&msg, "agent@mca.example", "MCA").is_err());
}

#[test]
fn recipients_lists_to_then_cc() {
    assert_eq!(recipients(&message()).len(), 2);
}
