//! The red flag for potential orders.
//!
//! One boolean on the email row, derived from signals the pipeline already
//! produces — never a second classification. These tests pin the contract
//! a manager relies on when scanning the inbox: commercial traffic is
//! flagged, ordinary mail, spam and machine notifications are not, a
//! follow-up stays tied to the order its thread started, an ambiguous
//! message is left alone, and a reprocess reaches the same answer without
//! duplicating anything.
//!
//! Set `MCA_TEST_DATABASE_URL` to run them; without it every test is
//! skipped. No real mailbox, no outbound mail, no network.

use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use mca_mail::api::{self, ApiState};
use mca_mail::config::{ApiSettings, AppConfig, EmailMode};
use mca_mail::domain::{EmailAddress, EmailStatus, InboundMessage};
use mca_mail::llm::mock::MockLlmProvider;
use mca_mail::llm::LlmProvider;
use mca_mail::orchestration::{Orchestrator, OrchestratorBuilder};
use mca_mail::persistence::{api_key_repo, email_repo, thread_repo};
use mca_mail::tools::ToolRegistry;
use mca_mail_testkit::replies;

/// One scenario at a time: a lead or an outbox row left by an earlier test
/// in this file would leak into the next one's thread context.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

async fn test_pool() -> Option<PgPool> {
    let url = std::env::var("MCA_TEST_DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .ok()?;
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    Some(pool)
}

fn test_database_url() -> String {
    std::env::var("MCA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://mca:mca@localhost:5432/mca_mail_test".to_string())
}

/// Review mode: the pipeline computes the flag exactly as in auto mode —
/// the flag is not an action, so the outbound policy is irrelevant here.
fn review_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.llm.disabled = true;
    cfg.database.url = test_database_url();
    cfg.security.email_mode = EmailMode::Review;
    cfg.security.manager_card.enabled = false;
    cfg
}

fn mock_llm_settings() -> mca_mail::config::LlmSettings {
    mca_mail::config::LlmSettings {
        provider: mca_mail::config::LlmProviderKind::Mock,
        ..Default::default()
    }
}

fn build_orchestrator(pool: PgPool, llm: Arc<dyn LlmProvider>) -> Arc<Orchestrator> {
    Arc::new(
        OrchestratorBuilder::new()
            .config(review_config())
            .pool(pool)
            .tools(ToolRegistry::new())
            .llm(llm)
            .build()
            .expect("build orchestrator"),
    )
}

async fn api(pg: PgPool) -> axum::Router {
    let state = Arc::new(ApiState::new(pg.clone()));
    api::build_router(&ApiSettings::default(), state)
}

async fn mint(pg: &PgPool, name: &str, role: api_key_repo::Role) -> String {
    let (raw, _) = api_key_repo::create(pg, name, role, "order-flag-tests", None)
        .await
        .expect("create key");
    raw
}

/// A sender unique per call: the thread key is (sender, normalized subject),
/// so a fresh address keeps this run's threads and leads away from whatever
/// an earlier run left in the shared test database.
fn fresh_address() -> String {
    format!("client-{}@example.com", uuid::Uuid::new_v4())
}

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

async fn ingest(pool: &PgPool, message: &InboundMessage) -> uuid::Uuid {
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

fn classification(category: &str, confidence: f32) -> Value {
    json!({
        "category": category,
        "confidence": confidence,
        "explanation": "order flag fixture",
        "requires_human": false,
        "suggested_action": "qualify"
    })
}

fn automated_notification() -> Value {
    json!({
        "verdict": "automated_notification",
        "confidence": 0.9,
        "explanation": "portal digest",
        "markers": []
    })
}

/// The whole turn: spam, classification, qualification and reply. Stubs are
/// keyed on each agent's system prompt, so a second turn calls `reset` and
/// re-stubs rather than trying to tell the turns apart by content.
fn stub_turn(llm: &MockLlmProvider, spam: &Value, class: &Value) {
    llm.stub_json("spam detection expert", spam);
    llm.stub_json("classification agent for a logistics", class);
    llm.stub_json(
        "lead qualification agent",
        &replies::qualification_transport(),
    );
    llm.stub_json(
        "customer manager at MCA Logistics",
        &replies::communication_draft(),
    );
}

async fn order_flag(pool: &PgPool, id: uuid::Uuid) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT order_flag FROM emails WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read order_flag")
}

async fn email_status(pool: &PgPool, id: uuid::Uuid) -> EmailStatus {
    sqlx::query_scalar::<_, String>("SELECT status FROM emails WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read status")
        .parse()
        .expect("parse status")
}

async fn lead_rows(pool: &PgPool, id: uuid::Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM leads WHERE primary_email_id = $1 OR thread_id = (SELECT thread_id FROM emails WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count leads")
}

async fn email_rows(pool: &PgPool, thread_id: uuid::Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM emails WHERE thread_id = $1")
        .bind(thread_id)
        .fetch_one(pool)
        .await
        .expect("count emails")
}

// ---------------------------------------------------------------------------
// 1. commercial traffic earns the flag
// ---------------------------------------------------------------------------

#[tokio::test]
async fn transport_request_is_flagged_and_shown_in_the_api() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn(
        &llm,
        &replies::not_spam(),
        &classification("transport_request", 0.9),
    );
    let orch = build_orchestrator(pg.clone(), llm);

    let id = ingest(
        &pg,
        &email(
            &fresh_address(),
            "Запрос на перевозку станков",
            "Нужна перевозка двух станков ЧПУ из Шэньчжэня в Москву, 1800 кг каждый.",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    assert!(
        order_flag(&pg, id).await,
        "a transport request must be flagged"
    );
    assert_eq!(email_status(&pg, id).await, EmailStatus::Processed);

    // The flag reaches the manager through the same API the list is read
    // from, sitting next to the subject.
    let key = mint(&pg, "flag-viewer", api_key_repo::Role::Viewer).await;
    let app = api(pg.clone()).await;
    let (_, detail) = axum_get(&app, &format!("/api/v1/emails/{id}"), &key).await;
    assert_eq!(detail["order_flag"], json!(true), "detail: {detail}");
    assert!(!detail["subject"].as_str().unwrap().is_empty());

    let thread_id = detail["thread_id"].as_str().expect("thread_id");
    let (_, thread) = axum_get(&app, &format!("/api/v1/emails/{id}/thread"), &key).await;
    for message in thread["messages"].as_array().expect("messages") {
        assert_eq!(message["order_flag"], json!(true), "thread: {thread_id}");
    }
}

#[tokio::test]
async fn price_quote_request_is_flagged() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn(
        &llm,
        &replies::not_spam(),
        &classification("new_lead", 0.85),
    );
    let orch = build_orchestrator(pg.clone(), llm);

    let id = ingest(
        &pg,
        &email(
            &fresh_address(),
            "Стоимость доставки",
            "Посчитайте, пожалуйста, стоимость доставки контейнера 40 feet в Санкт-Петербург.",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    assert!(
        order_flag(&pg, id).await,
        "a price question must be flagged"
    );
}

// ---------------------------------------------------------------------------
// 2. the thread keeps the order link
// ---------------------------------------------------------------------------

#[tokio::test]
async fn customer_reply_in_an_order_thread_is_flagged() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    let from = fresh_address();
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    let orch = build_orchestrator(pg.clone(), llm.clone());

    // Turn one: the request itself, confident demand.
    stub_turn(
        &llm,
        &replies::not_spam(),
        &classification("transport_request", 0.9),
    );
    let request = ingest(
        &pg,
        &email(
            &from,
            "Доставка груза",
            "Интересует морская доставка из Нинбо.",
        ),
    )
    .await;
    orch.process_email(request).await.expect("process request");
    assert!(order_flag(&pg, request).await);

    // Turn two: the client's follow-up. The model reads it as an existing
    // client but scores it below the auto-action threshold — the thread is
    // what has to carry the flag.
    llm.reset();
    stub_turn(
        &llm,
        &replies::not_spam(),
        &classification("existing_client", 0.6),
    );
    let reply = ingest(
        &pg,
        &email(
            &from,
            "Re: Доставка груза",
            "Груз готов, когда можно забирать?",
        ),
    )
    .await;
    orch.process_email(reply).await.expect("process reply");

    assert!(
        order_flag(&pg, reply).await,
        "a client's follow-up in an order thread must keep the flag"
    );
    assert_eq!(
        lead_rows(&pg, request).await,
        1,
        "one conversation, one lead — the reply must not open a second"
    );
}

#[tokio::test]
async fn invoice_in_an_order_thread_is_flagged() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    let from = fresh_address();
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    let orch = build_orchestrator(pg.clone(), llm.clone());

    stub_turn(
        &llm,
        &replies::not_spam(),
        &classification("transport_request", 0.9),
    );
    let request = ingest(
        &pg,
        &email(
            &from,
            "Отгрузка оборудования",
            "Нужна доставка оборудования в Москву.",
        ),
    )
    .await;
    orch.process_email(request).await.expect("process request");

    // The invoice is a neutral category on its own — it earns the flag only
    // because the thread around it is already an order.
    llm.reset();
    stub_turn(
        &llm,
        &replies::not_spam(),
        &classification("document_request", 0.9),
    );
    let invoice = ingest(
        &pg,
        &email(
            &from,
            "Re: Отгрузка оборудования",
            "Накладная во вложении, оплата по безналу.",
        ),
    )
    .await;
    orch.process_email(invoice).await.expect("process invoice");

    assert!(
        order_flag(&pg, invoice).await,
        "an invoice inside an order thread must be flagged"
    );
}

// ---------------------------------------------------------------------------
// 3. everything else stays unflagged
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ordinary_correspondence_is_not_flagged() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn(&llm, &replies::not_spam(), &classification("partner", 0.9));
    let orch = build_orchestrator(pg.clone(), llm);

    let id = ingest(
        &pg,
        &email(
            &fresh_address(),
            "Совместный вебинар",
            "Предлагаем провести совместный вебинар для ваших клиентов.",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    assert!(
        !order_flag(&pg, id).await,
        "ordinary correspondence must stay unflagged"
    );
}

#[tokio::test]
async fn spam_is_not_flagged() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn(&llm, &replies::spam(), &classification("new_lead", 0.9));
    let orch = build_orchestrator(pg.clone(), llm);

    let id = ingest(
        &pg,
        &email(
            &fresh_address(),
            "Выиграйте миллион!",
            "Казино, бонусы, срочно откройте счёт — перевозка тоже есть.",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    assert_eq!(email_status(&pg, id).await, EmailStatus::Quarantined);
    assert!(!order_flag(&pg, id).await, "spam must never be flagged");
    assert_eq!(lead_rows(&pg, id).await, 0, "spam must not open a lead");
}

#[tokio::test]
async fn machine_notification_is_not_flagged() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn(
        &llm,
        &automated_notification(),
        &classification("internal", 0.9),
    );
    let orch = build_orchestrator(pg.clone(), llm);

    let id = ingest(
        &pg,
        &email(
            &fresh_address(),
            "Новое отклики на вакансию",
            "У вас 3 новых отклика на вакансию менеджера.",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    assert!(
        !order_flag(&pg, id).await,
        "an automated notification must stay unflagged"
    );
}

#[tokio::test]
async fn ambiguous_email_is_not_flagged() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    // The model could not place the message at all.
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn(
        &llm,
        &replies::not_spam(),
        &classification("uncertain", 0.5),
    );
    let orch = build_orchestrator(pg.clone(), llm.clone());

    let unclear = ingest(
        &pg,
        &email(&fresh_address(), "По поводу", "Здравствуйте, уточню позже."),
    )
    .await;
    orch.process_email(unclear).await.expect("process unclear");
    assert!(
        !order_flag(&pg, unclear).await,
        "unclear mail stays unflagged"
    );

    // The model saw demand but was not sure of it, and the thread says
    // nothing: ambiguity must not be guessed through. The lead the
    // pipeline still creates is exactly why the flag is not the lead.
    llm.reset();
    stub_turn(
        &llm,
        &replies::not_spam(),
        &classification("transport_request", 0.5),
    );
    let guessed = ingest(
        &pg,
        &email(
            &fresh_address(),
            "Что-то про доставку",
            "Может быть, понадобится доставка, как-нибудь потом.",
        ),
    )
    .await;
    orch.process_email(guessed).await.expect("process guessed");
    assert!(
        !order_flag(&pg, guessed).await,
        "low-confidence demand without thread context must stay unflagged"
    );
}

// ---------------------------------------------------------------------------
// 4. the flag is stable under reprocessing and restarts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reprocessing_keeps_one_stable_flag() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn(
        &llm,
        &replies::not_spam(),
        &classification("transport_request", 0.9),
    );
    let orch = build_orchestrator(pg.clone(), llm);

    let id = ingest(
        &pg,
        &email(
            &fresh_address(),
            "Доставка реагентов",
            "Нужна доставка реагентов из Казани, 4 паллеты.",
        ),
    )
    .await;
    orch.process_email(id).await.expect("first process");
    assert!(order_flag(&pg, id).await);

    let thread_id =
        sqlx::query_scalar::<_, uuid::Uuid>("SELECT thread_id FROM emails WHERE id = $1")
            .bind(id)
            .fetch_one(&pg)
            .await
            .expect("thread id");
    let emails_before = email_rows(&pg, thread_id).await;

    // A reprocess starts from `pending` again — the same state a restart
    // and a mailbox rescan would produce.
    sqlx::query("UPDATE emails SET status = 'pending' WHERE id = $1")
        .bind(id)
        .execute(&pg)
        .await
        .expect("reset status");
    orch.process_email(id).await.expect("second process");

    assert!(
        order_flag(&pg, id).await,
        "a reprocess must reach the same verdict"
    );
    assert_eq!(
        email_rows(&pg, thread_id).await,
        emails_before,
        "a reprocess must not duplicate the message"
    );
    assert_eq!(
        lead_rows(&pg, id).await,
        1,
        "a reprocess must not open a second lead"
    );
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

async fn axum_get(app: &axum::Router, uri: &str, key: &str) -> (axum::http::StatusCode, Value) {
    use tower::ServiceExt;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("X-API-Key", key)
        .body(axum::body::Body::empty())
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}
