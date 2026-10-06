//! End-to-end pipeline tests with deterministic mocks.
//!
//! Requires `MCA_TEST_DATABASE_URL` (PostgreSQL). No real mail, no paid LLM:
//! the mock mail provider and mock LLM make every run reproducible.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use mca_mail::config::AppConfig;
use mca_mail::llm::mock::MockLlmProvider;
use mca_mail::mail::MockMailProvider;
use mca_mail::orchestration::{Orchestrator, OrchestratorBuilder};
use mca_mail::persistence::{email_repo, thread_repo};
use mca_mail::tools::ToolRegistry;

use mca_mail::domain::{EmailAddress, EmailStatus, InboundMessage};
use mca_mail_testkit::replies;

fn test_database_url() -> String {
    std::env::var("MCA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://mca:mca@localhost:5432/mca_mail_test".to_string())
}

async fn setup_db() -> sqlx::PgPool {
    let url = test_database_url();
    let pool = mca_mail::persistence::pool::connect(&{
        mca_mail::config::DatabaseSettings {
            url,
            auto_migrate: true,
            ..Default::default()
        }
    })
    .await
    .expect("connect to test database");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}

fn test_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.llm.disabled = true;
    cfg.database.url = test_database_url();
    cfg
}

fn mock_llm_settings() -> mca_mail::config::LlmSettings {
    mca_mail::config::LlmSettings {
        provider: mca_mail::config::LlmProviderKind::Mock,
        ..Default::default()
    }
}

fn build_orchestrator(
    pool: sqlx::PgPool,
    llm: Arc<dyn mca_mail::llm::LlmProvider>,
) -> Arc<Orchestrator> {
    build_orchestrator_with(test_config(), pool, llm, None)
}

/// `mailbox` is separate from the config because it is the only dependency a
/// test cannot express through settings: the handle is built once at boot.
fn build_orchestrator_with(
    config: AppConfig,
    pool: sqlx::PgPool,
    llm: Arc<dyn mca_mail::llm::LlmProvider>,
    mailbox: Option<Arc<dyn mca_mail::mail::MaybeWritable>>,
) -> Arc<Orchestrator> {
    let tools = ToolRegistry::new();
    Arc::new(
        OrchestratorBuilder::new()
            .config(config)
            .pool(pool)
            .tools(tools)
            .llm(llm)
            .mailbox(mailbox)
            .build()
            .expect("build orchestrator"),
    )
}

async fn ingest(pool: &sqlx::PgPool, message: &InboundMessage) -> mca_mail::domain::EmailId {
    let normalized = mca_mail::domain::normalize_subject(&message.subject);
    let key = thread_repo::conversation_key(&[&message.from], &normalized);
    let thread = thread_repo::ensure_thread(pool, &key, &normalized, None)
        .await
        .expect("thread");
    let outcome = email_repo::insert_inbound(pool, thread, "test", message, None)
        .await
        .expect("insert");
    outcome.email_id()
}

fn email(subject: &str, body: &str) -> InboundMessage {
    // A fresh provider id per call keeps re-runs against a long-lived database
    // from hitting the dedup path and skipping the pipeline.
    let nonce = uuid::Uuid::new_v4();
    InboundMessage {
        provider_message_id: format!("mock:{subject}:{nonce}"),
        internet_message_id: Some(format!("<{nonce}@test>")),
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::new("client@example.com"),
        to: vec![EmailAddress::new("sales@mca-logistics.ru")],
        cc: vec![],
        subject: subject.to_string(),
        date: Some(chrono::Utc::now()),
        text_body: body.to_string(),
        attachments: vec![],
        total_size: body.len(),
    }
}

#[tokio::test]
async fn spam_email_is_quarantined_not_deleted() {
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    llm.stub_json("Скидка", &replies::spam());

    // Moving a message out of the inbox is gated twice over: `MailMode` inside
    // the transport guard and `EMAIL_MODE=auto` in the policy. Both have to be
    // open or the message stays where it is, which is exactly what this test
    // would otherwise fail to notice.
    let mut config = test_config();
    config.security.email_mode = mca_mail::config::EmailMode::Auto;
    let mailbox = Arc::new(MockMailProvider::with_mode(
        mca_mail::config::MailMode::ReadWrite,
    ));
    let log = mailbox.mutation_log();
    let orch = build_orchestrator_with(
        config,
        pool.clone(),
        llm as Arc<dyn mca_mail::llm::LlmProvider>,
        Some(mailbox),
    );

    let id = ingest(&pool, &email("Скидка 70%", "Купите наши услуги")).await;
    // The UID is stamped by the poller in production; stamp it here so the
    // move has a destination the server would recognise.
    sqlx::query("UPDATE emails SET provider_uid = '91' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("stamp the server uid");
    orch.process_email(id).await.expect("process");

    let stored = email_repo::get(&pool, id).await.expect("get");
    assert_eq!(stored.status, EmailStatus::Quarantined);
    assert_eq!(
        stored.spam_verdict,
        Some(mca_mail::domain::SpamVerdict::Spam)
    );
    assert_eq!(
        log.quarantine.load(Ordering::SeqCst),
        1,
        "the message must be moved out of the inbox, not only flagged"
    );
}

#[tokio::test]
async fn commercial_email_creates_lead() {
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &replies::classification_lead(),
    );
    llm.stub_json(
        "lead qualification agent",
        &replies::qualification_transport(),
    );
    llm.stub_json(
        "customer manager at MCA Logistics",
        &replies::communication_draft(),
    );
    let spy = llm.clone();
    let orch = build_orchestrator(pool.clone(), llm as Arc<dyn mca_mail::llm::LlmProvider>);

    let id = ingest(
        &pool,
        &email(
            "Доставка из Китая",
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;
    if let Err(e) = orch.process_email(id).await {
        for (i, call) in spy.call_log().iter().enumerate() {
            eprintln!(
                "=== call {i} ===\n{}",
                call.chars().take(600).collect::<String>()
            );
        }
        panic!("process: {e:?}");
    }

    let stored = email_repo::get(&pool, id).await.expect("get");
    assert_eq!(stored.status, EmailStatus::Processed);
    assert!(stored.lead_id.is_some(), "lead must be attached");
}

#[tokio::test]
async fn reprocessing_does_not_duplicate() {
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &replies::classification_lead(),
    );
    llm.stub_json(
        "lead qualification agent",
        &replies::qualification_transport(),
    );
    llm.stub_json(
        "customer manager at MCA Logistics",
        &replies::communication_draft(),
    );
    let orch = build_orchestrator(pool.clone(), llm as Arc<dyn mca_mail::llm::LlmProvider>);

    let id = ingest(
        &pool,
        &email(
            "Доставка из Китая",
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;

    orch.process_email(id).await.expect("first process");
    orch.process_email(id).await.expect("second process");

    let stored = email_repo::get(&pool, id).await.expect("get");
    // Idempotent by dedup: the same message must not create a second run.
    let runs = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM email_processing_runs WHERE email_id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("count runs");
    assert!(runs <= 1, "reprocessing must not create duplicate runs");
    assert_eq!(stored.status, EmailStatus::Processed);
}
