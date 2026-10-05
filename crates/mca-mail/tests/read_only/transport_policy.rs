use mca_mail::config::{EmailMode, MailMode, OutboundPolicy};
use mca_mail::mail::{
    MailboxWriter, MockMailProvider, OutboundDecision, OutboundPolicyGuard, PolicyContext,
};

use super::super::fixtures::{is_refusal, outbound};

/// Two independent switches must both be thrown before mail leaves the
/// process: the transport guard (`MAIL_MODE`) and the send policy
/// (`EMAIL_MODE` + `EMAIL_AUTO_SEND`). Neither one alone is enough.
#[tokio::test]
async fn transport_and_policy_are_separate_switches() {
    let guard = OutboundPolicyGuard::new(
        EmailMode::Auto,
        OutboundPolicy {
            auto_send: false,
            ..Default::default()
        },
        false,
    );
    let context =
        PolicyContext::denied_by_default(EmailMode::Auto, "buyer@example.com", chrono::Utc::now())
            .with_body(100);
    assert!(!matches!(guard.evaluate(&context), OutboundDecision::Send));

    // Policy says yes, transport still says no.
    let read_only = MockMailProvider::with_mode(MailMode::ReadOnly);
    assert!(is_refusal(
        MailboxWriter::send(&read_only, &outbound()).await
    ));
    assert!(read_only.sent_messages().await.is_empty());

    // Transport says yes, policy still says no.
    let writable = MockMailProvider::with_mode(MailMode::Work);
    assert!(writable.guard().allows_write());
    assert!(!matches!(guard.evaluate(&context), OutboundDecision::Send));
}

/// With both switches thrown the message reaches the transport exactly once.
#[tokio::test]
async fn both_switches_reaching_the_transport_delivers_once() {
    let guard = OutboundPolicyGuard::new(
        EmailMode::Auto,
        OutboundPolicy {
            auto_send: true,
            ..Default::default()
        },
        false,
    );
    let context =
        PolicyContext::denied_by_default(EmailMode::Auto, "buyer@example.com", chrono::Utc::now())
            .with_body(100);
    assert!(matches!(guard.evaluate(&context), OutboundDecision::Send));

    let writable = MockMailProvider::with_mode(MailMode::Work);
    MailboxWriter::send(&writable, &outbound())
        .await
        .expect("delivered");
    assert_eq!(writable.sent_messages().await.len(), 1);
}
