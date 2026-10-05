//! The transport half of read-only mode, with no database involved.
//!
//! These are the assertions that would catch a guard being moved, bypassed or
//! counted wrongly, and they hold whether or not a mailbox is reachable.

use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;

use mca_mail::config::MailMode;
use mca_mail::domain::{EmailAddress, InboundMessage, MailboxIdentity};
use mca_mail::mail::{MailProvider, MailboxWriter, MockMailProvider};

use super::fixtures::{outbound, seeded_read_only};
use crate::support;

#[path = "transport_policy.rs"]
mod policy;

/// The mutation counter is the audit trail: nothing may be applied.
#[tokio::test]
async fn read_only_applies_no_mailbox_mutation() {
    let provider = MockMailProvider::with_mode(MailMode::ReadOnly);
    let log = provider.mutation_log();

    let _ = MailboxWriter::send(&provider, &outbound()).await;
    let _ = MailboxWriter::append_draft(&provider, &outbound()).await;
    let _ = MailboxWriter::append_sent(&provider, &outbound()).await;
    let _ = MailboxWriter::move_to_role(&provider, 1, mca_mail::mail::folders::FolderRole::Archive)
        .await;
    let _ = MailboxWriter::delete_message(&provider, 1).await;
    let _ = MailboxWriter::mark_processed(&provider, 1).await;
    let _ = MailboxWriter::quarantine(&provider, 1, "spam").await;

    assert_eq!(log.applied.load(SeqCst), 0, "no mutation may be applied");
    let attempted = log.send.load(SeqCst)
        + log.append_draft.load(SeqCst)
        + log.append_sent.load(SeqCst)
        + log.move_message.load(SeqCst)
        + log.delete_message.load(SeqCst)
        + log.mark_processed.load(SeqCst)
        + log.quarantine.load(SeqCst);
    assert_eq!(attempted, 7, "every call must be counted as attempted");
    assert_eq!(
        log.refused.load(SeqCst),
        7,
        "every call must be recorded as refused"
    );
}

/// Reading in read-only mode does not consume the message, so a crash before
/// the database commit cannot lose it. The real provider gets this from
/// `BODY.PEEK`; the mock must behave the same, or a read-only test would pass
/// only because the mock had quietly diverged from the transport.
#[tokio::test]
async fn read_only_fetch_does_not_consume_the_message() {
    let provider = seeded_read_only();
    assert_eq!(provider.guard().mode(), MailMode::ReadOnly);
    assert_eq!(provider.fetch_new().await.expect("first fetch").len(), 1);
    assert_eq!(
        provider.fetch_new().await.expect("second fetch").len(),
        1,
        "read-only must not consume a message it is not allowed to flag"
    );
}

/// The corpus constructor honours the mode instead of defaulting to work.
#[test]
fn the_directory_constructor_honours_read_only() {
    let dir = support::temp_dir("read-only-corpus").display().to_string();
    let provider = MockMailProvider::from_corpus_dir(dir, MailMode::ReadOnly);
    let log = provider.mutation_log();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(async {
        let _ = provider.mark_processed(1).await;
    });
    assert_eq!(log.applied.load(SeqCst), 0);
    assert_eq!(log.refused.load(SeqCst), 1);
}

/// A read-only provider is a full reader: the trait is split, not truncated.
#[test]
fn read_only_is_a_full_reader() {
    let provider: Arc<dyn MailProvider> = Arc::new(MockMailProvider::with_mode(MailMode::ReadOnly));
    let _ = provider.name();
    let _ = MailboxIdentity::default();
    let _ = InboundMessage {
        provider_message_id: "p".into(),
        internet_message_id: None,
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::new("a@example.com"),
        to: vec![],
        cc: vec![],
        subject: "s".into(),
        date: None,
        text_body: "b".into(),
        attachments: vec![],
        total_size: 1,
    };
}
