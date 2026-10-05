//! Fresh-mail discovery against a real PostgreSQL: the UIDVALIDITY +
//! high-water-mark cursor that decides what counts as new.
//!
//! Requires `MCA_TEST_DATABASE_URL`; without it the module is skipped rather
//! than silently passing. No real mailbox is involved: `MockMailProvider`
//! plays the server and assigns UIDs the way a server does.

// The shared helper carries a scratch-dir helper this binary does not use;
// suppressing the warning here keeps the workspace clippy-clean.
#[allow(dead_code)]
mod support;

use mca_mail::application::workers::poll::poll_once;
use mca_mail::domain::{EmailAddress, InboundMessage};
use mca_mail::error::MailError;
use mca_mail::mail::MockMailProvider;

fn message(subject: &str) -> InboundMessage {
    let nonce = uuid::Uuid::new_v4();
    InboundMessage {
        provider_message_id: format!("mock:{nonce}"),
        internet_message_id: Some(format!("<{nonce}@test>")),
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::new("client@example.com"),
        to: vec![EmailAddress::new("inbox@mca.example")],
        cc: vec![],
        subject: subject.to_string(),
        date: None,
        text_body: format!("body {subject} {nonce}"),
        attachments: vec![],
        total_size: 32,
    }
}

fn three_messages() -> Vec<InboundMessage> {
    vec![message("old-1"), message("old-2"), message("old-3")]
}

async fn stored_count(pool: &sqlx::PgPool, mailbox: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM emails WHERE mailbox = $1")
        .bind(mailbox)
        .fetch_one(pool)
        .await
        .expect("count emails")
}

async fn subjects(pool: &sqlx::PgPool, mailbox: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT subject FROM emails WHERE mailbox = $1")
        .bind(mailbox)
        .fetch_all(pool)
        .await
        .expect("list subjects")
}

async fn cursor(pool: &sqlx::PgPool, mailbox: &str) -> Option<(i64, i64)> {
    sqlx::query_as("SELECT uid_validity, high_water_uid FROM mailbox_cursors WHERE mailbox = $1")
        .bind(mailbox)
        .fetch_optional(pool)
        .await
        .expect("read cursor")
}

/// Simulate a crash between "stored" and "mark advanced" by rewinding the mark.
async fn rewind_mark(pool: &sqlx::PgPool, mailbox: &str, high_water_uid: i64) {
    sqlx::query("UPDATE mailbox_cursors SET high_water_uid = $2 WHERE mailbox = $1")
        .bind(mailbox)
        .bind(high_water_uid)
        .execute(pool)
        .await
        .expect("rewind mark");
}

/// Scenario: first run records the boundary at UIDNEXT-1 and processes none of
/// the history; only mail arriving afterwards is ingested, with the mailbox's
/// UIDVALIDITY stored on the row.
#[tokio::test]
async fn first_run_bounds_existing_mail_and_only_new_arrives() {
    let Some(pool) = support::pool().await else {
        return;
    };
    let mailbox = support::unique("cursor-first-run");
    let provider = MockMailProvider::with_messages(three_messages());

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("first poll");
    assert_eq!(
        stored_count(&pool, &mailbox).await,
        0,
        "existing history must not be processed on the first run"
    );
    assert_eq!(cursor(&pool, &mailbox).await, Some((1, 3)));

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("second poll");
    assert_eq!(stored_count(&pool, &mailbox).await, 0);

    let uid = provider.enqueue(message("fresh")).await;
    assert_eq!(uid, 4, "the new message lands after the boundary");
    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("third poll");
    assert_eq!(stored_count(&pool, &mailbox).await, 1);
    assert_eq!(cursor(&pool, &mailbox).await, Some((1, 4)));

    let validity: Option<i64> =
        sqlx::query_scalar("SELECT provider_uid_validity FROM emails WHERE mailbox = $1")
            .bind(&mailbox)
            .fetch_one(&pool)
            .await
            .expect("row validity");
    assert_eq!(validity, Some(1), "UIDVALIDITY is stored per message row");
}

/// Scenario: a restart resumes from the saved boundary instead of re-reading
/// the mailbox from the beginning.
#[tokio::test]
async fn restart_resumes_from_the_saved_boundary() {
    let Some(pool) = support::pool().await else {
        return;
    };
    let mailbox = support::unique("cursor-restart");

    let first = MockMailProvider::with_messages(three_messages());
    poll_once(&first, &pool, &mailbox).await.expect("first run");
    drop(first);

    // A fresh provider instance stands in for a new process: same server
    // state, same database — only the cursor decides what counts as new.
    let second = MockMailProvider::with_messages(three_messages());
    poll_once(&second, &pool, &mailbox)
        .await
        .expect("second run");
    assert_eq!(
        stored_count(&pool, &mailbox).await,
        0,
        "a restart must not replay history"
    );

    second.enqueue(message("after-restart")).await;
    poll_once(&second, &pool, &mailbox)
        .await
        .expect("third run");
    assert_eq!(stored_count(&pool, &mailbox).await, 1);
}

/// Scenario: a crash (or a repeated delivery) before the watermark commit
/// re-fetches the same range — the dedup key must absorb the replay.
#[tokio::test]
async fn replayed_fetch_is_deduplicated_not_duplicated() {
    let Some(pool) = support::pool().await else {
        return;
    };
    let mailbox = support::unique("cursor-replay");
    let provider = MockMailProvider::with_messages(three_messages());

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("boundary poll");
    provider.enqueue(message("only-once")).await;
    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("ingest poll");
    assert_eq!(stored_count(&pool, &mailbox).await, 1);

    rewind_mark(&pool, &mailbox, 3).await;
    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("replay poll");
    assert_eq!(
        stored_count(&pool, &mailbox).await,
        1,
        "the replayed fetch must resolve to the existing row"
    );
    assert_eq!(cursor(&pool, &mailbox).await, Some((1, 4)));
}

/// Scenario: a UIDVALIDITY change re-bounds the mailbox at the current edge
/// without backfilling anything that exists under the new numbering.
#[tokio::test]
async fn uidvalidity_change_rebounds_without_backfill() {
    let Some(pool) = support::pool().await else {
        return;
    };
    let mailbox = support::unique("cursor-rebind");
    let provider = MockMailProvider::with_messages(three_messages());

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("boundary poll");
    provider.enqueue(message("mid-change")).await;
    provider.set_uid_validity(2);

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("rebind poll");
    assert_eq!(
        cursor(&pool, &mailbox).await,
        Some((2, 4)),
        "the mark is rewritten under the new validity"
    );
    assert_eq!(
        stored_count(&pool, &mailbox).await,
        0,
        "nothing that existed at rebind time may be backfilled"
    );

    provider.enqueue(message("after-rebind")).await;
    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("post-rebind poll");
    let stored = subjects(&pool, &mailbox).await;
    assert_eq!(stored, vec!["after-rebind".to_string()]);
    assert_eq!(cursor(&pool, &mailbox).await, Some((2, 5)));
}

/// Scenario: the batch limit cuts the *oldest* new mail first (UID SEARCH
/// returns an unordered set — the sort before the limit is what guarantees
/// this), and the mark resumes exactly where the batch stopped.
#[tokio::test]
async fn batch_limit_keeps_oldest_first_and_resumes() {
    let Some(pool) = support::pool().await else {
        return;
    };
    let mailbox = support::unique("cursor-batch");
    let provider = MockMailProvider::with_messages(vec![]);

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("empty-mailbox boundary");
    assert_eq!(cursor(&pool, &mailbox).await, Some((1, 0)));

    provider.set_fetch_limit(2);
    for (i, tag) in ["a", "b", "c", "d", "e"].into_iter().enumerate() {
        let uid = provider.enqueue(message(tag)).await;
        assert_eq!(uid as usize, i + 1, "UIDs are assigned in arrival order");
    }

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("batch 1");
    let first = subjects(&pool, &mailbox).await;
    assert_eq!(first.len(), 2, "one batch of two");
    assert!(
        first.iter().all(|s| s == "a" || s == "b"),
        "the oldest two must come first, got {first:?}"
    );
    assert_eq!(cursor(&pool, &mailbox).await, Some((1, 2)));

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("batch 2");
    assert_eq!(stored_count(&pool, &mailbox).await, 4);
    assert_eq!(cursor(&pool, &mailbox).await, Some((1, 4)));

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("batch 3");
    assert_eq!(stored_count(&pool, &mailbox).await, 5);
    assert_eq!(cursor(&pool, &mailbox).await, Some((1, 5)));

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("drained");
    assert_eq!(stored_count(&pool, &mailbox).await, 5);
}

/// Scenario: a transient network failure must neither move the mark nor lose
/// the message; the next cycle fetches it again and stores it once.
#[tokio::test]
async fn a_failed_fetch_loses_nothing_and_resumes() {
    let Some(pool) = support::pool().await else {
        return;
    };
    let mailbox = support::unique("cursor-fetch-failure");
    let provider = MockMailProvider::with_messages(three_messages());

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("boundary poll");
    provider.enqueue(message("survivor")).await;

    provider
        .fail_next_fetch(MailError::Unavailable("network down".into()))
        .await;
    poll_once(&provider, &pool, &mailbox)
        .await
        .expect_err("the failed cycle must surface its error");
    assert_eq!(stored_count(&pool, &mailbox).await, 0);
    assert_eq!(
        cursor(&pool, &mailbox).await,
        Some((1, 3)),
        "a failed cycle must not move the mark"
    );

    poll_once(&provider, &pool, &mailbox)
        .await
        .expect("retry cycle");
    assert_eq!(stored_count(&pool, &mailbox).await, 1);
    assert_eq!(cursor(&pool, &mailbox).await, Some((1, 4)));
}
