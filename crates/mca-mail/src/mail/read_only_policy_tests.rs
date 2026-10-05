use super::*;
use crate::config::MailSettings;
use std::sync::atomic::Ordering::SeqCst;

#[tokio::test]
async fn read_only_refuses_flag_changes() {
    let provider = mock(MailMode::ReadOnly);
    let log = provider.mutation_log();
    assert!(is_refusal(
        &provider.set_flag(7, "\\Seen").await.unwrap_err()
    ));
    assert!(is_refusal(
        &provider.clear_flag(7, "\\Seen").await.unwrap_err()
    ));
    assert!(is_refusal(&provider.mark_processed(7).await.unwrap_err()));
    assert!(is_refusal(
        &provider.quarantine(7, "spam").await.unwrap_err()
    ));
    assert_eq!(log.set_flag.load(SeqCst), 1);
    assert_eq!(log.clear_flag.load(SeqCst), 1);
    assert_eq!(log.mark_processed.load(SeqCst), 1);
    assert_eq!(log.quarantine.load(SeqCst), 1);
    assert_eq!(log.applied.load(SeqCst), 0);
}

#[tokio::test]
async fn read_write_permits_only_safe_mailbox_operations() {
    let provider = mock(MailMode::ReadWrite);
    let log = provider.mutation_log();
    provider
        .move_to_role(7, crate::mail::folders::FolderRole::Archive)
        .await
        .expect("move allowed");
    provider.set_flag(7, "\\Seen").await.expect("flag allowed");
    assert!(provider.delete_message(7).await.is_err());
    assert!(provider.set_flag(7, "\\Deleted").await.is_err());
    assert!(provider.append_draft(&outbound()).await.is_err());
    // Archiving a message that already went out is the one APPEND read-write
    // permits: it cannot reach anyone, and without it the reply never shows up
    // in the customer's sent folder.
    provider
        .append_sent(&outbound())
        .await
        .expect("archiving a delivered message allowed");
    // SMTP is reachable from `read_write`, but only through the transport
    // guard: the send decision itself still belongs to `OutboundPolicyGuard`.
    provider.send(&outbound()).await.expect("send allowed");
    assert_eq!(log.applied.load(SeqCst), 4);
    assert_eq!(provider.sent_messages().await.len(), 1);
}

#[test]
fn work_mode_still_obeys_the_outbound_policy() {
    use crate::config::EmailMode;
    use crate::mail::policy::{OutboundDecision, OutboundPolicyGuard, PolicyContext};

    let guard = OutboundPolicyGuard::new(
        EmailMode::Auto,
        crate::config::OutboundPolicy {
            auto_send: false,
            ..Default::default()
        },
        false,
    );
    let context =
        PolicyContext::denied_by_default(EmailMode::Auto, "c@example.com", chrono::Utc::now())
            .with_body(100);
    assert!(matches!(
        guard.evaluate(&context),
        OutboundDecision::Draft(_)
    ));
    assert!(MailMode::ReadWrite.allows_mailbox_write());
}

#[tokio::test]
async fn mode_survives_provider_restart_and_defaults_to_read_only() {
    let settings = MailSettings {
        mode: MailMode::ReadOnly,
        ..Default::default()
    };
    for _ in 0..3 {
        let provider = MockMailProvider::with_mode(settings.mode);
        assert!(is_refusal(&provider.send(&outbound()).await.unwrap_err()));
    }
    assert_eq!(MailSettings::default().mode, MailMode::ReadOnly);
}
