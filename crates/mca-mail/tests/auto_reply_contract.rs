//! Stage G: the production auto-responder.
//!
//! Four claims have to hold together before this may touch a real customer:
//! a reply the pipeline produced actually leaves the building, review mode
//! still holds the very same reply, a qualified lead produces one manager
//! card to every configured address, and spam never reaches the outbox
//! at all. The fifth is idempotency: one inbound email, one queued reply.

use std::sync::Arc;

use mca_mail::application::workers::outbox_loop;
use mca_mail::config::{AppConfig, EmailMode, MailMode};
use mca_mail::domain::{EmailAddress, EmailStatus, InboundMessage, LeadId};
use mca_mail::llm::mock::MockLlmProvider;
use mca_mail::llm::LlmProvider;
use mca_mail::mail::{MaybeWritable, MockMailProvider};
use mca_mail::orchestration::{Orchestrator, OrchestratorBuilder};
use mca_mail::persistence::{email_repo, thread_repo};
use mca_mail::tools::ToolRegistry;
use mca_mail_testkit::replies;

/// One shared claim queue: a worker started here would otherwise deliver a
/// row another test in this file just queued.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const MANAGER: &str = "savva.toch@gmail.com";
/// A second manager on the same card: one address each, one copy each.
const CO_MANAGER: &str = "parkin@mca-log.com";

fn test_database_url() -> String {
    std::env::var("MCA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://mca:mca@localhost:5432/mca_mail_test".to_string())
}

async fn setup_db() -> sqlx::PgPool {
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
    // The database outlives a failed run. A row left `queued` by an earlier
    // pass would be claimed by the worker started below and would add a send
    // this test never made.
    sqlx::query("DELETE FROM mailbox_outbox WHERE status IN ('queued', 'sending')")
        .execute(&pool)
        .await
        .expect("clear leftover test rows");
    pool
}

/// The settings a deployment reaches by setting the six env switches. Rate
/// limits are wide open: they are covered by the policy unit tests, and here
/// they would only make assertions depend on what earlier runs left behind.
fn auto_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.llm.disabled = true;
    cfg.database.url = test_database_url();
    cfg.security.email_mode = EmailMode::Auto;
    cfg.security.outbound.auto_send = true;
    cfg.security.outbound.max_sends_per_hour = 1_000_000;
    cfg.security.outbound.max_sends_per_lead_per_hour = 1_000_000;
    cfg.security.outbound.min_interval_seconds = 0;
    cfg.security.manager_card.enabled = true;
    cfg.security.manager_card.recipient = format!("{MANAGER},{CO_MANAGER}");
    cfg.security.manager_card.max_per_hour = 1_000_000;
    cfg
}

/// `auto_send` is open but the mode is not: the gate that must hold the reply
/// back is the mode, and only that one.
fn review_config() -> AppConfig {
    let mut cfg = auto_config();
    cfg.security.email_mode = EmailMode::Review;
    cfg
}

fn mock_llm_settings() -> mca_mail::config::LlmSettings {
    mca_mail::config::LlmSettings {
        provider: mca_mail::config::LlmProviderKind::Mock,
        ..Default::default()
    }
}

/// `mailbox` is separate from the config because it is the only dependency a
/// test cannot express through settings: the handle is built once at boot.
fn build_orchestrator(
    config: AppConfig,
    pool: sqlx::PgPool,
    llm: Arc<dyn LlmProvider>,
) -> Arc<Orchestrator> {
    Arc::new(
        OrchestratorBuilder::new()
            .config(config)
            .pool(pool)
            .tools(ToolRegistry::new())
            .llm(llm)
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
    email_repo::insert_inbound(pool, thread, "test", message, None)
        .await
        .expect("insert")
        .email_id()
}

/// A per-run sender keeps the thread key, the lead and the rate limiter from
/// colliding with a run this file made earlier against the same database.
fn email(from: &str, subject: &str, body: &str) -> InboundMessage {
    let nonce = uuid::Uuid::new_v4();
    InboundMessage {
        provider_message_id: format!("mock:{subject}:{nonce}"),
        internet_message_id: Some(format!("<{nonce}@test>")),
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::new(from),
        to: vec![EmailAddress::new("sales@mca-logistics.ru")],
        cc: vec![],
        subject: subject.to_string(),
        date: Some(chrono::Utc::now()),
        text_body: body.to_string(),
        attachments: vec![],
        total_size: body.len(),
    }
}

fn stub_pipeline(
    llm: &MockLlmProvider,
    qualification: &serde_json::Value,
    reply: &serde_json::Value,
) {
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &replies::classification_lead(),
    );
    llm.stub_json("lead qualification agent", qualification);
    llm.stub_json("customer manager at MCA Logistics", reply);
}

/// The model extracted nothing: every quote-blocking field is still a gap,
/// which is why the reply asks a question instead of closing the dialogue.
fn qualification_partial() -> serde_json::Value {
    replies::qualification_transport()
}

fn req(field: &str, value: &str, unit: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "field": field,
        "value": value,
        "state": "known",
        "unit": unit,
        "confidence": 0.95,
        "evidence": value
    })
}

/// Everything a quote needs, so `blocking_gaps` comes back empty and the lead
/// becomes a manager's job the moment we reply.
fn qualification_complete() -> serde_json::Value {
    serde_json::json!({
        "lead_id": null,
        "first_email_id": null,
        "company_name": "ООО Рейс",
        "contact_name": "Иван Петров",
        "contact_phone": "+7 (495) 123-45-67",
        "summary": "Перевозка станков из Китая в Россию",
        "scope": "transport",
        "needs_expert": false,
        "questions": [],
        "confidence": 0.9,
        "regulated_topics": [],
        "requirements": [
            req("goods_name", "станки", None),
            req("origin_country", "Китай", None),
            req("destination_country", "Россия", None),
            req("goods_weight", "12000", Some("kg")),
            req("goods_quantity", "2", Some("шт")),
            req("goods_volume", "30", Some("м3")),
            req("desired_deadline", "до конца месяца", None),
            req("transport_mode", "авто", None)
        ]
    })
}

/// A reply that asks for the weight — `draft` on purpose, so the test proves
/// the server dispatches it when auto-send is open rather than waiting for a
/// reviewer who is not there.
fn reply_asking() -> serde_json::Value {
    serde_json::json!({
        "subject": "Re: Доставка станков",
        "body": "Здравствуйте! Уточните, пожалуйста, вес и объём груза.",
        "disposition": "draft",
        "questions": ["Уточните, пожалуйста, вес и объём груза?"],
        "handoff_requested": false,
        "handoff_reason": null,
        "confidence": 0.9,
        "rationale": "параметры груза ещё неизвестны"
    })
}

/// A closing reply: nothing missing, nothing to ask, the manager takes over.
fn reply_complete() -> serde_json::Value {
    serde_json::json!({
        "subject": "Re: Доставка станков",
        "body": "Здравствуйте! Информация полна, менеджер свяжется с вами.",
        "disposition": "send",
        "questions": [],
        "handoff_requested": false,
        "handoff_reason": null,
        "confidence": 0.9,
        "rationale": "все данные собраны"
    })
}

/// `(message_type, recipient, status)` of every outbound row for a lead.
async fn outbox_for_lead(pool: &sqlx::PgPool, lead_id: LeadId) -> Vec<(String, String, String)> {
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT message_type, recipient, status FROM mailbox_outbox \
         WHERE lead_id = $1 ORDER BY created_at",
    )
    .bind(lead_id)
    .fetch_all(pool)
    .await
    .expect("read outbox")
}

/// Status of the one row this lead has for `kind` and `recipient`.
async fn outbox_status(
    pool: &sqlx::PgPool,
    lead_id: LeadId,
    kind: &str,
    recipient: &str,
) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT status FROM mailbox_outbox \
         WHERE lead_id = $1 AND message_type = $2 AND recipient = $3",
    )
    .bind(lead_id)
    .bind(kind)
    .bind(recipient)
    .fetch_one(pool)
    .await
    .expect("outbox row")
}

async fn drafts_for(pool: &sqlx::PgPool, lead_id: LeadId) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM email_drafts WHERE lead_id = $1")
        .bind(lead_id)
        .fetch_one(pool)
        .await
        .expect("count drafts")
}

/// Outbound rows an inbound message produced, whatever their recipient.
async fn outbox_for_email(pool: &sqlx::PgPool, email_id: uuid::Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mailbox_outbox WHERE email_id = $1")
        .bind(email_id)
        .fetch_one(pool)
        .await
        .expect("count outbox")
}

async fn sent_to(mock: &MockMailProvider, recipient: &str) -> usize {
    mock.sent_messages()
        .await
        .iter()
        .filter(|record| record.to.iter().any(|address| address == recipient))
        .count()
}

async fn run_worker(pool: &sqlx::PgPool, writer: Arc<dyn MaybeWritable>, cfg: AppConfig) {
    let (signal, rx) = mca_mail::shutdown::Signal::new();
    let handle = tokio::spawn(outbox_loop(pool.clone(), writer, cfg, rx));
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    signal.stop();
    handle.await.expect("outbox loop");
}

fn writer() -> Arc<MockMailProvider> {
    Arc::new(MockMailProvider::with_mode(MailMode::Work))
}

#[tokio::test]
async fn auto_send_dispatches_the_reply_the_model_held_back() {
    let _guard = SERIAL.lock().await;
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_pipeline(&llm, &qualification_partial(), &reply_asking());
    let orch = build_orchestrator(auto_config(), pool.clone(), llm as Arc<dyn LlmProvider>);

    let buyer = format!("buyer-{}@example.com", uuid::Uuid::new_v4());
    let id = ingest(
        &pool,
        &email(
            &buyer,
            &format!("Доставка станков {}", uuid::Uuid::new_v4()),
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    let lead_id = email_repo::get(&pool, id)
        .await
        .expect("get")
        .lead_id
        .expect("a lead must open");
    let queued = outbox_for_lead(&pool, lead_id).await;
    assert_eq!(
        queued,
        vec![(
            "customer_reply".to_string(),
            buyer.clone(),
            "queued".to_string()
        )],
        "auto mode must put the reply in the outbox addressed to the sender"
    );

    let mock = writer();
    let as_writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pool, as_writer, auto_config()).await;

    assert_eq!(sent_to(&mock, &buyer).await, 1, "exactly one delivery");
    assert_eq!(
        outbox_status(&pool, lead_id, "customer_reply", &buyer).await,
        "sent"
    );
}

#[tokio::test]
async fn review_mode_keeps_the_same_reply_as_a_draft() {
    let _guard = SERIAL.lock().await;
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_pipeline(&llm, &qualification_partial(), &reply_complete());
    let orch = build_orchestrator(review_config(), pool.clone(), llm as Arc<dyn LlmProvider>);

    let buyer = format!("buyer-{}@example.com", uuid::Uuid::new_v4());
    let id = ingest(
        &pool,
        &email(
            &buyer,
            &format!("Доставка станков {}", uuid::Uuid::new_v4()),
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    let lead_id = email_repo::get(&pool, id)
        .await
        .expect("get")
        .lead_id
        .expect("a lead must open");
    assert_eq!(
        drafts_for(&pool, lead_id).await,
        1,
        "the reply is still prepared"
    );
    assert_eq!(
        outbox_for_lead(&pool, lead_id).await,
        Vec::<(String, String, String)>::new(),
        "review mode must not let the same reply out"
    );
}

#[tokio::test]
async fn a_qualified_lead_queues_one_manager_card_per_configured_address() {
    let _guard = SERIAL.lock().await;
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_pipeline(&llm, &qualification_complete(), &reply_complete());
    let orch = build_orchestrator(auto_config(), pool.clone(), llm as Arc<dyn LlmProvider>);

    let buyer = format!("buyer-{}@example.com", uuid::Uuid::new_v4());
    let id = ingest(
        &pool,
        &email(
            &buyer,
            &format!("Доставка станков {}", uuid::Uuid::new_v4()),
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    let lead_id = email_repo::get(&pool, id)
        .await
        .expect("get")
        .lead_id
        .expect("a lead must open");
    let mut queued = outbox_for_lead(&pool, lead_id).await;
    queued.sort();
    let mut expected = vec![
        (
            "customer_reply".to_string(),
            buyer.clone(),
            "queued".to_string(),
        ),
        (
            "manager_card".to_string(),
            MANAGER.to_string(),
            "queued".to_string(),
        ),
        (
            "manager_card".to_string(),
            CO_MANAGER.to_string(),
            "queued".to_string(),
        ),
    ];
    expected.sort();
    assert_eq!(
        queued, expected,
        "a completed lead must queue one reply and a card to every manager"
    );

    let mock = writer();
    let as_writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pool, as_writer, auto_config()).await;

    assert_eq!(sent_to(&mock, &buyer).await, 1, "the reply went out");
    assert_eq!(
        sent_to(&mock, MANAGER).await,
        1,
        "exactly one card reached the first manager"
    );
    assert_eq!(
        sent_to(&mock, CO_MANAGER).await,
        1,
        "exactly one card reached the second manager"
    );
    for manager in [MANAGER, CO_MANAGER] {
        assert_eq!(
            outbox_status(&pool, lead_id, "manager_card", manager).await,
            "sent",
            "the card to {manager} was delivered"
        );
    }

    // One card per lead and address, ever: the same lead going through the
    // pipeline again must collapse onto the keys it already used.
    orch.process_email(id).await.expect("reprocess");
    assert_eq!(
        outbox_for_lead(&pool, lead_id).await.len(),
        3,
        "reprocessing must not queue a second card for any manager"
    );
}

#[tokio::test]
async fn spam_never_reaches_the_outbox() {
    let _guard = SERIAL.lock().await;
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    llm.stub_json("Скидка", &replies::spam());
    let orch = build_orchestrator(auto_config(), pool.clone(), llm as Arc<dyn LlmProvider>);

    let id = ingest(
        &pool,
        &email(
            "spammer@example.com",
            &format!("Скидка 70% {}", uuid::Uuid::new_v4()),
            "Купите наши услуги",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    let stored = email_repo::get(&pool, id).await.expect("get");
    assert_eq!(stored.status, EmailStatus::Quarantined);
    assert_eq!(
        outbox_for_email(&pool, id).await,
        0,
        "a convicted message must never produce an outbound row"
    );
}

#[tokio::test]
async fn one_inbound_email_queues_exactly_one_reply() {
    let _guard = SERIAL.lock().await;
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_pipeline(&llm, &qualification_partial(), &reply_asking());
    let orch = build_orchestrator(auto_config(), pool.clone(), llm as Arc<dyn LlmProvider>);

    let buyer = format!("buyer-{}@example.com", uuid::Uuid::new_v4());
    let id = ingest(
        &pool,
        &email(
            &buyer,
            &format!("Доставка станков {}", uuid::Uuid::new_v4()),
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;

    orch.process_email(id).await.expect("first process");
    orch.process_email(id).await.expect("second process");

    assert_eq!(
        outbox_for_email(&pool, id).await,
        1,
        "a replayed message must not queue a second reply"
    );
}
