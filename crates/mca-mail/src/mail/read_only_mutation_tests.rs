use super::*;
use std::sync::atomic::Ordering::SeqCst;

#[tokio::test]
async fn read_only_refuses_smtp_send() {
    let provider = mock(MailMode::ReadOnly);
    let log = provider.mutation_log();
    assert!(is_refusal(&provider.send(&outbound()).await.unwrap_err()));
    assert_eq!(log.send.load(SeqCst), 1);
    assert_eq!(log.applied.load(SeqCst), 0);
    assert!(provider.sent_messages().await.is_empty());
}

#[tokio::test]
async fn read_only_refuses_imap_append_draft() {
    let provider = mock(MailMode::ReadOnly);
    let log = provider.mutation_log();
    assert!(is_refusal(
        &provider.append_draft(&outbound()).await.unwrap_err()
    ));
    assert_eq!(log.append_draft.load(SeqCst), 1);
    assert_eq!(log.applied.load(SeqCst), 0);
}

#[tokio::test]
async fn read_only_refuses_archiving_a_delivered_message() {
    let provider = mock(MailMode::ReadOnly);
    let log = provider.mutation_log();
    assert!(is_refusal(
        &provider.append_sent(&outbound()).await.unwrap_err()
    ));
    assert_eq!(log.append_sent.load(SeqCst), 1);
    assert_eq!(log.applied.load(SeqCst), 0);
}

#[tokio::test]
async fn read_only_refuses_move_copy_and_delete() {
    let provider = mock(MailMode::ReadOnly);
    let log = provider.mutation_log();
    assert!(is_refusal(
        &provider
            .move_to_role(7, crate::mail::folders::FolderRole::Archive)
            .await
            .unwrap_err()
    ));
    assert!(is_refusal(
        &provider
            .copy_to_role(7, crate::mail::folders::FolderRole::Archive)
            .await
            .unwrap_err()
    ));
    assert!(is_refusal(&provider.delete_message(7).await.unwrap_err()));
    assert_eq!(log.move_message.load(SeqCst), 1);
    assert_eq!(log.copy_message.load(SeqCst), 1);
    assert_eq!(log.delete_message.load(SeqCst), 1);
    assert_eq!(log.applied.load(SeqCst), 0);
}

#[tokio::test]
async fn refused_mutations_never_reach_the_transport() {
    let provider = mock(MailMode::ReadOnly);
    let log = provider.mutation_log();
    let _ = provider.send(&outbound()).await;
    let _ = provider.append_draft(&outbound()).await;
    let _ = provider.append_sent(&outbound()).await;
    let _ = provider
        .move_to_role(1, crate::mail::folders::FolderRole::Other)
        .await;
    let _ = provider
        .copy_to_role(1, crate::mail::folders::FolderRole::Other)
        .await;
    let _ = provider.delete_message(1).await;
    let _ = provider.set_flag(1, "\\Seen").await;
    let _ = provider.clear_flag(1, "\\Seen").await;
    let _ = provider.mark_processed(1).await;
    let _ = provider.quarantine(1, "reason").await;
    assert_eq!(log.total(), 10);
    assert_eq!(log.applied.load(SeqCst), 0);
}
