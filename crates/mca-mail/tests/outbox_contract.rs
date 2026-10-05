//! Stage E: what leaves the outbox, and what does not.
//!
//! The contract under test is narrow but load-bearing: one queued row becomes
//! exactly one delivery, a second pass finds nothing to claim for it, and a
//! policy that says no stops a message that is already queued. That last part
//! is why the check happens in the worker rather than at enqueue time.

use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;

use mca_mail::application::workers::outbox_loop;
use mca_mail::config::{AppConfig, EmailMode, MailMode};
use mca_mail::mail::{MaybeWritable, MockMailProvider};
use mca_mail::persistence::lead_repo;
use mca_mail::persistence::outbox_repo::{self, OutboundIntent, OutboundKind};

/// These tests share one database and one claim queue: a worker started here
/// would otherwise send a row another test in this file just queued.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn test_database_url() -> String {
    std::env::var("MCA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://mca:mca@localhost:5432/mca_mail_test".to_string())
}

async fn pool() -> sqlx::PgPool {
    let pool = mca_mail::persistence::pool::connect(&mca_mail::config::DatabaseSettings {
        url: test_database_url(),
        auto_migrate: true,
        ..Default::default()
    })
    .await
    .expect("connect to test database");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    // The database outlives a failed run; rows it left behind are picked up by
    // the next worker and would show up as deliveries this test never made.
    sqlx::query("DELETE FROM mailbox_outbox WHERE idempotency_key LIKE 'test:%'")
        .execute(&pool)
        .await
        .expect("clear leftover test rows");
    pool
}

/// Auto mode with the throttle opened up: rate limits are covered by the
/// policy unit tests, and here they would only make the assertions depend on
/// what earlier runs left behind in a database that outlives the test.
fn auto_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.security.email_mode = EmailMode::Auto;
    cfg.security.outbound.auto_send = true;
    cfg.security.outbound.max_sends_per_hour = 1_000_000;
    cfg.security.outbound.max_sends_per_lead_per_hour = 1_000_000;
    cfg.security.outbound.min_interval_seconds = 0;
    cfg
}

fn intent(recipient: &str, key: &str) -> OutboundIntent {
    OutboundIntent {
        message_type: OutboundKind::CustomerReply,
        recipient: recipient.to_string(),
        subject: "Здравствуйте".into(),
        body_text: "Ответ на ваш запрос".into(),
        body_html: "<p>Ответ на ваш запрос</p>".into(),
        idempotency_key: key.to_string(),
        ..OutboundIntent::default()
    }
}

/// Unique per run: a repeated address would trip the pacing check that
/// belongs to a different test.
fn fresh_recipient() -> String {
    format!("buyer-{}@example.com", uuid::Uuid::new_v4())
}

async fn row_status(pool: &sqlx::PgPool, key: &str) -> (String, String, i32) {
    sqlx::query_as::<_, (String, String, i32)>(
        "SELECT status, COALESCE(policy_denial, ''), attempts FROM mailbox_outbox \
         WHERE idempotency_key = $1",
    )
    .bind(key)
    .fetch_one(pool)
    .await
    .expect("outbox row")
}

/// Deliveries of this test's message, ignoring whatever an earlier run left.
async fn sent_to(mock: &MockMailProvider, recipient: &str) -> usize {
    mock.sent_messages()
        .await
        .iter()
        .filter(|record| record.to.iter().any(|address| address == recipient))
        .count()
}

/// Run the worker for long enough to drain the queue, then stop it.
async fn run_worker(pool: &sqlx::PgPool, writer: Arc<dyn MaybeWritable>, cfg: AppConfig) {
    let (signal, rx) = mca_mail::shutdown::Signal::new();
    let handle = tokio::spawn(outbox_loop(pool.clone(), writer, cfg, rx));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    signal.stop();
    handle.await.expect("outbox loop");
}

fn writer() -> Arc<MockMailProvider> {
    Arc::new(MockMailProvider::with_mode(MailMode::Work))
}

#[tokio::test]
async fn a_queued_message_becomes_exactly_one_delivery() {
    let _guard = SERIAL.lock().await;
    let pool = pool().await;
    let recipient = fresh_recipient();
    let key = format!("test:send:{}", uuid::Uuid::new_v4());

    outbox_repo::enqueue_send(&pool, &intent(&recipient, &key))
        .await
        .expect("enqueue")
        .expect("a new row must be created");

    let mock = writer();
    let as_writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pool, as_writer, auto_config()).await;

    assert_eq!(sent_to(&mock, &recipient).await, 1, "exactly one send");
    let (status, _, attempts) = row_status(&pool, &key).await;
    assert_eq!(status, "sent");
    assert_eq!(attempts, 1);

    // A second pass is what a restarted worker would look like: the row is
    // already `sent`, so it is never claimed again.
    let as_writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pool, as_writer, auto_config()).await;
    assert_eq!(sent_to(&mock, &recipient).await, 1, "no second delivery");
    let (status, _, attempts) = row_status(&pool, &key).await;
    assert_eq!(status, "sent");
    assert_eq!(attempts, 1);
}

#[tokio::test]
async fn dry_run_holds_a_queued_message_instead_of_sending_it() {
    let _guard = SERIAL.lock().await;
    let pool = pool().await;
    let recipient = fresh_recipient();
    let key = format!("test:dry:{}", uuid::Uuid::new_v4());

    outbox_repo::enqueue_send(&pool, &intent(&recipient, &key))
        .await
        .expect("enqueue");

    let mock = writer();
    let as_writer: Arc<dyn MaybeWritable> = mock.clone();
    let mut cfg = auto_config();
    cfg.security.email_mode = EmailMode::DryRun;
    run_worker(&pool, as_writer, cfg).await;

    assert_eq!(
        sent_to(&mock, &recipient).await,
        0,
        "dry run must not reach the transport"
    );
    let (status, denial, attempts) = row_status(&pool, &key).await;
    assert_eq!(status, "held");
    assert_eq!(denial, "mode_is_not_auto");
    // Held is terminal: the reason stays on the row and it is never claimed.
    assert_eq!(attempts, 0);
}

#[tokio::test]
async fn the_same_idempotency_key_never_queues_twice() {
    let _guard = SERIAL.lock().await;
    let pool = pool().await;
    let recipient = fresh_recipient();
    let key = format!("test:dupe:{}", uuid::Uuid::new_v4());

    let first = outbox_repo::enqueue_send(&pool, &intent(&recipient, &key))
        .await
        .expect("first enqueue");
    let second = outbox_repo::enqueue_send(&pool, &intent(&recipient, &key))
        .await
        .expect("second enqueue");

    assert!(first.is_some(), "the first enqueue must create a row");
    assert!(second.is_none(), "the second enqueue must be a no-op");
}

#[tokio::test]
async fn manager_cards_wait_while_their_switch_is_off() {
    let _guard = SERIAL.lock().await;
    let pool = pool().await;
    let recipient = fresh_recipient();
    let key = format!("test:card:{}", uuid::Uuid::new_v4());
    let mut card = intent(&recipient, &key);
    card.message_type = OutboundKind::ManagerCard;
    outbox_repo::enqueue_send(&pool, &card)
        .await
        .expect("enqueue");

    // Auto mode would send a customer reply, but the card has its own switch
    // and it is off.
    let mut cfg = auto_config();
    cfg.security.manager_card.enabled = false;
    let mock = writer();
    let as_writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pool, as_writer, cfg).await;

    assert_eq!(
        sent_to(&mock, &recipient).await,
        0,
        "the card's switch is off, nothing may leave"
    );
    let (status, denial, _) = row_status(&pool, &key).await;
    assert_eq!(status, "held");
    assert_eq!(denial, "manager_card_disabled");
}

/// One card per lead *per manager*: another address gets its own copy, the
/// same address never gets a second one — even when the row already holding
/// the card was queued under the older key that carried no address.
#[tokio::test]
async fn the_manager_card_is_unique_per_lead_and_recipient() {
    let _guard = SERIAL.lock().await;
    let pool = pool().await;
    let lead = lead_repo::create_manual(&pool, &fresh_recipient(), None, None)
        .await
        .expect("lead");
    let first = fresh_recipient();

    let mut card = intent(&first, &format!("test:card:{}", uuid::Uuid::new_v4()));
    card.message_type = OutboundKind::ManagerCard;
    card.lead_id = Some(lead);
    outbox_repo::enqueue_send(&pool, &card)
        .await
        .expect("first card")
        .expect("a new card must be created");

    // Same manager, a different key: the index, not the key, has the last word.
    let mut again = intent(&first, &format!("test:card:{}", uuid::Uuid::new_v4()));
    again.message_type = OutboundKind::ManagerCard;
    again.lead_id = Some(lead);
    assert!(
        outbox_repo::enqueue_send(&pool, &again)
            .await
            .expect("second card")
            .is_none(),
        "the same manager must not receive a second card for one lead"
    );

    // A second manager is a different recipient, so it is a different card.
    let second = fresh_recipient();
    let mut other = intent(&second, &format!("test:card:{}", uuid::Uuid::new_v4()));
    other.message_type = OutboundKind::ManagerCard;
    other.lead_id = Some(lead);
    outbox_repo::enqueue_send(&pool, &other)
        .await
        .expect("third card")
        .expect("another manager must get their own card");
}

/// A delivered message must also land in the sent folder: that copy is the
/// only proof, inside the customer's own client, that a reply went out.
///
/// Archiving happens after the message is gone, so it is attempted once per
/// delivery — and never for a message that did not leave.
#[tokio::test]
async fn a_delivered_message_is_archived_in_the_sent_folder() {
    let _guard = SERIAL.lock().await;
    let pool = pool().await;
    let recipient = fresh_recipient();
    let key = format!("test:archive:{}", uuid::Uuid::new_v4());

    outbox_repo::enqueue_send(&pool, &intent(&recipient, &key))
        .await
        .expect("enqueue")
        .expect("a new row must be created");

    let mock = writer();
    let log = mock.mutation_log();
    let as_writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pool, as_writer, auto_config()).await;

    assert_eq!(sent_to(&mock, &recipient).await, 1, "exactly one send");
    assert_eq!(
        log.append_sent.load(SeqCst),
        1,
        "the delivered message must be APPENDed to the sent folder"
    );
    assert_eq!(row_status(&pool, &key).await.0, "sent");

    // A restarted worker claims nothing, so it archives nothing either.
    let as_writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pool, as_writer, auto_config()).await;
    assert_eq!(log.append_sent.load(SeqCst), 1, "no second archive");
}

/// A message policy holds is not archived: nothing left, so there is nothing
/// to show anyone in "Отправленные".
#[tokio::test]
async fn a_held_message_is_not_archived() {
    let _guard = SERIAL.lock().await;
    let pool = pool().await;
    let recipient = fresh_recipient();
    let key = format!("test:archive-held:{}", uuid::Uuid::new_v4());

    outbox_repo::enqueue_send(&pool, &intent(&recipient, &key))
        .await
        .expect("enqueue");

    let mock = writer();
    let log = mock.mutation_log();
    let mut cfg = auto_config();
    cfg.security.email_mode = EmailMode::DryRun;
    let as_writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pool, as_writer, cfg).await;

    assert_eq!(sent_to(&mock, &recipient).await, 0, "nothing may leave");
    assert_eq!(log.append_sent.load(SeqCst), 0, "nothing to archive");
    assert_eq!(row_status(&pool, &key).await.0, "held");
}
