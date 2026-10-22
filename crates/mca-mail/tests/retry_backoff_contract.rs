//! Integration tests against a real PostgreSQL: transient failures must back
//! off instead of hot-looping, and must stop coming back once the attempts
//! are spent.
//!
//! Set `MCA_TEST_DATABASE_URL` to run them, e.g.
//! `MCA_TEST_DATABASE_URL=postgres://mca:mca@localhost:55432/mca_mail cargo test`.
//! Without it the whole module is skipped rather than silently passing.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use mca_mail::config::DatabaseSettings;
use mca_mail::domain::{EmailAddress, InboundMessage};
use mca_mail::persistence;
use mca_mail::persistence::email_repo::{claim_batch, record_error, release, MAX_ATTEMPTS};

async fn pool() -> Option<PgPool> {
    let url = std::env::var("MCA_TEST_DATABASE_URL").ok()?;
    let settings = DatabaseSettings {
        url,
        max_connections: 4,
        min_connections: 0,
        auto_migrate: false,
        ..Default::default()
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&settings.url)
        .await
        .ok()?;
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    Some(pool)
}

fn message() -> InboundMessage {
    let nonce = uuid::Uuid::new_v4();
    InboundMessage {
        provider_message_id: format!("pmid-{nonce}"),
        internet_message_id: Some(format!("<{nonce}@example.com>")),
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::new("retry@example.com"),
        to: vec![EmailAddress::new("inbox@mca.example")],
        cc: vec![],
        subject: format!("retry backoff {nonce}"),
        date: None,
        text_body: "body".into(),
        attachments: vec![],
        total_size: 4,
    }
}

async fn insert_email(pool: &PgPool) -> uuid::Uuid {
    let msg = message();
    let normalized = mca_mail::domain::normalize_subject(&msg.subject);
    let thread_id = persistence::thread_repo::ensure_thread(
        pool,
        &persistence::thread_repo::conversation_key(&[&msg.from], &normalized),
        &normalized,
        msg.internet_message_id.as_deref(),
    )
    .await
    .expect("thread");
    persistence::email_repo::insert_inbound(pool, thread_id, "INBOX", &msg, None)
        .await
        .expect("insert")
        .email_id()
}

async fn claim_ids(pool: &PgPool) -> Vec<uuid::Uuid> {
    claim_batch(pool, 500, MAX_ATTEMPTS)
        .await
        .expect("claim")
        .into_iter()
        .map(|row| row.id)
        .collect()
}

/// The `touch_updated_at` trigger stamps every UPDATE with `now()`, which is
/// exactly what the backoff needs in production and exactly what a test must
/// bypass to simulate a minute of waiting. `session_replication_role` turns
/// the trigger off for this connection only.
async fn age(pool: &PgPool, id: uuid::Uuid) {
    let mut conn = pool.acquire().await.expect("connection");
    sqlx::query("SET session_replication_role = replica")
        .execute(&mut *conn)
        .await
        .expect("disarm triggers");
    sqlx::query("UPDATE emails SET updated_at = now() - interval '10 minutes' WHERE id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await
        .expect("age row");
    sqlx::query("RESET session_replication_role")
        .execute(&mut *conn)
        .await
        .expect("rearm triggers");
}

/// A released failure waits out the backoff before the queue may claim it
/// again, and once the attempts are spent it never comes back at all.
#[tokio::test]
async fn transient_failures_back_off_then_stop() {
    let Some(pool) = pool().await else {
        return;
    };
    let id = insert_email(&pool).await;

    // Simulate the pipeline's transient failure path: counter, then release.
    let attempts = record_error(&pool, id, "llm provider returned HTTP 429")
        .await
        .expect("record");
    assert_eq!(attempts, 1);
    release(&pool, id, "llm provider returned HTTP 429")
        .await
        .expect("release");

    // Fresh failure: the backoff holds it out of the queue...
    let claimed = claim_ids(&pool).await;
    assert!(!claimed.contains(&id), "claimed too early: {claimed:?}");

    // ...until the backoff has elapsed, when it is eligible again.
    age(&pool, id).await;
    let claimed = claim_ids(&pool).await;
    assert!(claimed.contains(&id), "still held back after backoff");

    // Spend the remaining attempts; each cycle re-enters the backoff.
    for _ in 1..MAX_ATTEMPTS {
        let attempts = record_error(&pool, id, "llm provider returned HTTP 429: again")
            .await
            .expect("record");
        release(&pool, id, "llm provider returned HTTP 429: again")
            .await
            .expect("release");
        age(&pool, id).await;
        let claimed = claim_ids(&pool).await;
        if attempts < MAX_ATTEMPTS {
            assert!(claimed.contains(&id), "attempt {attempts} never retried");
        }
    }

    // Attempts spent: even a long backoff never brings it back.
    let claimed = claim_ids(&pool).await;
    assert!(
        !claimed.contains(&id),
        "claimed past MAX_ATTEMPTS: {claimed:?}"
    );
    let row: (String, i32) = sqlx::query_as("SELECT status, attempts FROM emails WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(row.0, "failed");
    assert_eq!(row.1, MAX_ATTEMPTS);
}
