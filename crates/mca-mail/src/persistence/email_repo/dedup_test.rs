use crate::domain::{EmailAddress, InboundMessage};

use super::{dedup_key_for, scoped_dedup_key_for};

fn message(uid: &str, id: Option<&str>, body: &str) -> InboundMessage {
    InboundMessage {
        provider_message_id: uid.into(),
        internet_message_id: id.map(str::to_string),
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::new("client@example.test"),
        to: vec![],
        cc: vec![],
        subject: "Quote request".into(),
        date: None,
        text_body: body.into(),
        attachments: vec![],
        total_size: body.len(),
    }
}

#[test]
fn mailbox_uid_changes_do_not_create_a_second_logical_message() {
    assert_eq!(
        dedup_key_for(&message("uid-1", Some("<a@b.test>"), "body")),
        dedup_key_for(&message("uid-99", Some("<a@b.test>"), "body"))
    );
}

#[test]
fn duplicate_message_ids_with_different_contents_do_not_collide() {
    assert_ne!(
        dedup_key_for(&message("uid-1", Some("<a@b.test>"), "first")),
        dedup_key_for(&message("uid-2", Some("<a@b.test>"), "second"))
    );
}

#[test]
fn missing_message_id_falls_back_to_content_not_uid() {
    assert_eq!(
        dedup_key_for(&message("uid-1", None, "body")),
        dedup_key_for(&message("uid-2", None, "body"))
    );
}

#[test]
fn mailbox_identity_scopes_duplicate_detection() {
    let message = message("uid-1", Some("<a@b.test>"), "body");
    assert_ne!(
        scoped_dedup_key_for("INBOX", &message),
        scoped_dedup_key_for("Archive", &message)
    );
}
