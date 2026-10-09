//! The twenty acceptance scenarios for the workspace (§8).
//!
//! One named, runnable claim per scenario, covering both halves of the
//! system and the seam between them: the voice REST channel, the email
//! pipeline, and the shared database/outbox they meet on. External services
//! are mocked throughout — `MockLlmProvider` answers the model calls,
//! `MockMailProvider` plays SMTP, and no network, no real mailbox and no
//! real key is touched. Synthetic per-run senders and caller numbers keep
//! the scenarios independent of each other and of earlier runs.
//!
//! Set `MCA_TEST_DATABASE_URL` to run them; without it every scenario is
//! skipped. One scenario at a time: a worker started here must never
//! deliver a row another scenario just queued.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;

use mca_mail::api::{self, ApiState};
use mca_mail::application::workers::outbox_loop;
use mca_mail::config::{ApiSettings, AppConfig, DatabaseSettings, EmailMode, MailMode};
use mca_mail::domain::{EmailAddress, EmailStatus, InboundMessage, LeadId, RequirementField};
use mca_mail::llm::mock::MockLlmProvider;
use mca_mail::llm::LlmProvider;
use mca_mail::mail::{MaybeWritable, MockMailProvider};
use mca_mail::orchestration::{Orchestrator, OrchestratorBuilder};
use mca_mail::persistence::{api_key_repo, email_repo, requirement_repo, thread_repo};
use mca_mail::tools::ToolRegistry;
use mca_mail_testkit::replies;

/// One scenario at a time: a worker started by one scenario must not
/// deliver a row another scenario just queued.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

static PHONE_SEQ: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// shared fixtures
// ---------------------------------------------------------------------------

async fn test_pool() -> Option<PgPool> {
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

async fn api(pg: PgPool) -> axum::Router {
    let state = Arc::new(ApiState::new(pg.clone()));
    api::build_router(&ApiSettings::default(), state)
}

async fn mint(pg: &PgPool, name: &str, role: api_key_repo::Role) -> String {
    let (raw, _) = api_key_repo::create(pg, name, role, "scenarios", None)
        .await
        .expect("create key");
    raw
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(k) = key {
        builder = builder.header("X-API-Key", k);
    }
    let body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// 11 digits starting with `7` — the shape the `8…` → `7…` continuation
/// normalization rewrites; time keeps runs apart, the counter keeps
/// scenarios in one run apart.
fn fresh_phone() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis()
        % 10_000_000;
    let seq = PHONE_SEQ.fetch_add(1, Ordering::Relaxed) % 100;
    format!("79{stamp:07}{seq:02}")
}

async fn create_lead(app: &axum::Router, key: &str, phone: &str) -> Value {
    let (status, body) = call(
        app,
        "POST",
        "/api/v1/voice/leads",
        Some(key),
        Some(json!({ "caller_number": phone })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create lead: {body}");
    body
}

async fn fill_gaps(app: &axum::Router, key: &str, lead_id: &str) {
    let (_, requirements) = call(
        app,
        "GET",
        &format!("/api/v1/voice/leads/{lead_id}/requirements"),
        Some(key),
        None,
    )
    .await;
    let gaps: Vec<Value> = requirements["blocking_gaps"]
        .as_array()
        .expect("blocking_gaps")
        .clone();
    assert!(!gaps.is_empty(), "a fresh lead must have gaps to close");
    let facts: Vec<Value> = gaps
        .iter()
        .map(|gap| json!({ "field": gap["field"], "value": "12" }))
        .collect();
    let (status, body) = call(
        app,
        "PUT",
        &format!("/api/v1/voice/leads/{lead_id}/requirements"),
        Some(key),
        Some(json!({ "requirements": facts })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "save requirements: {body}");
    assert_eq!(body["blocking_gaps"], json!([]), "gaps must be closed");
}

async fn append_message(app: &axum::Router, key: &str, lead_id: &str, direction: &str, body: &str) {
    let (status, response) = call(
        app,
        "POST",
        &format!("/api/v1/voice/leads/{lead_id}/conversation"),
        Some(key),
        Some(json!({
            "direction": direction,
            "body": body,
            "idempotency_key": uuid::Uuid::new_v4().to_string(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "append `{body}`: {response}");
}

async fn finish_call(app: &axum::Router, key: &str, lead_id: &str, call_id: &str) -> Value {
    let (status, body) = call(
        app,
        "POST",
        "/api/v1/voice/calls/finish",
        Some(key),
        Some(json!({
            "lead_id": lead_id,
            "call_id": call_id,
            "outcome": "completed",
            "summary": "Клиент описал перевозку",
            "transcript": [
                { "direction": "user", "body": "Нужна перевозка" },
                { "direction": "assistant", "body": "Сколько груза?" },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "finish: {body}");
    body
}

/// Latest order-card audit row for a lead: `(outcome, reason)`.
async fn card_audit(pool: &PgPool, lead_id: &str) -> Option<(String, Option<String>)> {
    sqlx::query_as(
        "SELECT outcome, details->>'reason' FROM audit_logs \
         WHERE action = 'voice_order_card' AND resource_id = $1 \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(lead_id)
    .fetch_optional(pool)
    .await
    .expect("audit query")
}

/// Rows an inbound email produced, or a lead's rows: `(type, recipient, status)`.
async fn outbox_rows(pool: &PgPool, predicate: &str, id: Uuid) -> Vec<(String, String, String)> {
    sqlx::query_as::<_, (String, String, String)>(&format!(
        "SELECT message_type, recipient, status FROM mailbox_outbox \
         WHERE {predicate} ORDER BY created_at"
    ))
    .bind(id)
    .fetch_all(pool)
    .await
    .expect("read outbox")
}

type Uuid = uuid::Uuid;

async fn drafts_for(pool: &PgPool, lead_id: LeadId) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM email_drafts WHERE lead_id = $1")
        .bind(lead_id)
        .fetch_one(pool)
        .await
        .expect("count drafts")
}

async fn sent_to(mock: &MockMailProvider, recipient: &str) -> Vec<(String, usize)> {
    mock.sent_messages()
        .await
        .iter()
        .filter(|record| record.to.iter().any(|address| address == recipient))
        .map(|record| (record.subject.clone(), record.body_length))
        .collect()
}

async fn run_worker(pool: &PgPool, writer: Arc<dyn MaybeWritable>, cfg: AppConfig) {
    let (signal, rx) = mca_mail::shutdown::Signal::new();
    let handle = tokio::spawn(outbox_loop(pool.clone(), writer, cfg, rx));
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    signal.stop();
    handle.await.expect("outbox loop");
}

/// Rows another run left behind would be claimed by the worker started here.
async fn clear_pending(pool: &PgPool) {
    sqlx::query("DELETE FROM mailbox_outbox WHERE status IN ('queued', 'sending')")
        .execute(pool)
        .await
        .expect("clear leftover test rows");
}

// ---------------------------------------------------------------------------
// email-pipeline fixtures (mock LLM, mock mail)
// ---------------------------------------------------------------------------

fn test_database_url() -> String {
    std::env::var("MCA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://mca:mca@localhost:5432/mca_mail_test".to_string())
}

fn llm_settings() -> mca_mail::config::LlmSettings {
    mca_mail::config::LlmSettings {
        provider: mca_mail::config::LlmProviderKind::Mock,
        ..Default::default()
    }
}

fn base_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.llm.disabled = true;
    cfg.database.url = test_database_url();
    cfg
}

/// The production auto-responder settings, rate limits wide open (they are
/// policy territory and would only make assertions depend on leftovers).
fn auto_config() -> AppConfig {
    let mut cfg = base_config();
    cfg.security.email_mode = EmailMode::Auto;
    cfg.security.outbound.auto_send = true;
    cfg.security.outbound.max_sends_per_hour = 1_000_000;
    cfg.security.outbound.max_sends_per_lead_per_hour = 1_000_000;
    cfg.security.outbound.min_interval_seconds = 0;
    cfg.security.manager_card.enabled = false;
    cfg
}

fn review_config() -> AppConfig {
    let mut cfg = auto_config();
    cfg.security.email_mode = EmailMode::Review;
    cfg
}

fn build_orchestrator(
    pool: PgPool,
    llm: Arc<dyn LlmProvider>,
    cfg: AppConfig,
) -> Arc<Orchestrator> {
    Arc::new(
        OrchestratorBuilder::new()
            .config(cfg)
            .pool(pool)
            .tools(ToolRegistry::new())
            .llm(llm)
            .build()
            .expect("build orchestrator"),
    )
}

/// A per-run sender keeps the thread key, the lead and the rate limiter
/// from colliding with a run this file made earlier against the same
/// database.
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

/// The four agents of one turn. Stubs are keyed on each agent's system
/// prompt, so a second turn must `reset` and re-stub rather than trying to
/// tell the turns apart by content.
fn stub_pipeline(llm: &MockLlmProvider, qualification: &Value, reply: &Value) {
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &replies::classification_lead(),
    );
    llm.stub_json("lead qualification agent", qualification);
    llm.stub_json("customer manager at MCA Logistics", reply);
}

fn qualification_partial() -> Value {
    replies::qualification_transport()
}

/// Asks for the weight — a draft on purpose: scenario 11 proves the
/// auto-responder dispatches exactly that.
fn reply_asking() -> Value {
    json!({
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

// ===========================================================================
// A. The voice channel (REST against a real database)
// ===========================================================================

/// §8.1 — a first-time caller opens a lead that is seeded and has gaps:
/// the agent has something to ask about from the very first second.
#[tokio::test]
async fn scenario_01_new_caller_opens_a_seeded_lead() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;

    let body = create_lead(&app, &key, &fresh_phone()).await;
    assert_eq!(body["created"], true, "{body}");
    assert_eq!(body["status"], "new");
    assert_eq!(body["scope"], "full_import");
    let lead = body["lead_id"].as_str().expect("lead_id");

    let (status, requirements) = call(
        &app,
        "GET",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "requirements: {requirements}");
    assert!(
        !requirements["blocking_gaps"]
            .as_array()
            .expect("gaps")
            .is_empty(),
        "a fresh lead must have quote-blocking gaps"
    );
    assert_eq!(requirements["asked_questions"], json!([]));
}

/// §8.2 — the second call continues the first: the same subscriber lands
/// on the same lead in both dial formats, a stranger gets a new one.
#[tokio::test]
async fn scenario_02_repeat_caller_resumes_the_same_lead() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;

    let phone = fresh_phone();
    let first = create_lead(&app, &key, &phone).await;

    let again = create_lead(&app, &key, &phone).await;
    assert_eq!(again["created"], false, "same number, same lead: {again}");
    assert_eq!(again["lead_id"], first["lead_id"]);

    // The same human dialled with the trunk `8` today.
    let trunk_eight = format!("8{}", &phone[1..]);
    let same = create_lead(&app, &key, &trunk_eight).await;
    assert_eq!(
        same["created"], false,
        "8… and +7… are one subscriber: {same}"
    );
    assert_eq!(same["lead_id"], first["lead_id"]);

    let stranger = create_lead(&app, &key, &fresh_phone()).await;
    assert_eq!(stranger["created"], true);
    assert_ne!(stranger["lead_id"], first["lead_id"]);
}

/// §8.3 — the call agent only proposes: unknown fields and impossible
/// numbers are refused at the wire, valid facts reach the lead.
#[tokio::test]
async fn scenario_03_facts_are_validated_before_storage() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;
    let lead = create_lead(&app, &key, &fresh_phone()).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();

    let (status, body) = call(
        &app,
        "PUT",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        Some(json!({ "requirements": [{ "field": "horsepower", "value": "1" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "unknown field: {body}");

    let (status, body) = call(
        &app,
        "PUT",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        Some(json!({ "requirements": [{ "field": "goods_weight", "value": "-5" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "negative weight: {body}");

    let (status, body) = call(
        &app,
        "PUT",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        Some(json!({ "requirements": [
            { "field": "goods_weight", "value": "1200", "unit": "кг" },
            { "field": "goods_name", "value": "запчасти" }
        ] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "valid facts: {body}");
    assert_eq!(body["saved"], 2);

    let (_, requirements) = call(
        &app,
        "GET",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        None,
    )
    .await;
    let weight = requirements["requirements"]
        .as_array()
        .expect("requirements")
        .iter()
        .find(|item| item["field"] == "goods_weight")
        .expect("goods_weight row");
    assert_eq!(weight["value"], "1200");
    assert_eq!(weight["state"], "known");
}

/// §8.4 — gaps close as the call progresses; the list the agent reads is
/// live, not a snapshot from call setup.
#[tokio::test]
async fn scenario_04_blocking_gaps_close_as_the_call_progresses() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;
    let lead = create_lead(&app, &key, &fresh_phone()).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();

    let (_, before) = call(
        &app,
        "GET",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        None,
    )
    .await;
    let blocking_before = before["blocking_gaps"].as_array().expect("gaps").len();
    let open_before = before["open_gaps"].as_array().expect("open_gaps").len();
    assert!(blocking_before > 0, "a fresh lead must have blocking gaps");
    assert!(open_before >= blocking_before, "open is the wider list");

    fill_gaps(&app, &key, &lead).await;

    let (_, after) = call(
        &app,
        "GET",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        None,
    )
    .await;
    assert_eq!(after["blocking_gaps"], json!([]));
    let open_after = after["open_gaps"].as_array().expect("open_gaps").len();
    assert!(
        open_after < open_before,
        "every closed blocking gap leaves the open list: {open_before} → {open_after}"
    );
}

/// §8.5 — the continuation state the call agent reads: what it already
/// asked, in order, deduplicated, each with its answered flag.
#[tokio::test]
async fn scenario_05_agent_sees_what_it_already_asked() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;
    let lead = create_lead(&app, &key, &fresh_phone()).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();

    append_message(&app, &key, &lead, "outbound", "Здравствуйте, записываю.").await;
    append_message(&app, &key, &lead, "outbound", "Сколько груза?").await;
    append_message(&app, &key, &lead, "outbound", "сколько груза?").await;
    append_message(&app, &key, &lead, "inbound", "Двенадцать тонн.").await;
    append_message(&app, &key, &lead, "outbound", "Откуда забирать?").await;

    let (status, requirements) = call(
        &app,
        "GET",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "requirements: {requirements}");
    let asked = requirements["asked_questions"]
        .as_array()
        .expect("asked_questions");
    assert_eq!(
        asked.len(),
        2,
        "greeting dropped, duplicate collapsed: {asked:#?}"
    );
    assert_eq!(asked[0]["question"], "Сколько груза?");
    assert_eq!(asked[0]["answered"], true);
    assert_eq!(asked[1]["question"], "Откуда забирать?");
    assert_eq!(asked[1]["answered"], false);
}

/// §8.6 — qualification is the server's decision, not the model's: no
/// lead qualifies while a quote-blocking gap remains, all qualify after.
#[tokio::test]
async fn scenario_06_qualification_is_gated_on_blocking_gaps() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;
    let lead = create_lead(&app, &key, &fresh_phone()).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/v1/voice/leads/{lead}/qualify"),
        Some(&key),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "qualify: {body}");
    assert_eq!(body["qualified"], false, "gaps must block qualification");
    assert!(!body["blocking_gaps"].as_array().expect("gaps").is_empty());

    fill_gaps(&app, &key, &lead).await;

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/v1/voice/leads/{lead}/qualify"),
        Some(&key),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "qualify: {body}");
    assert_eq!(body["qualified"], true, "closed gaps must qualify: {body}");
    assert_eq!(body["blocking_gaps"], json!([]));
}

/// §8.7 — no email, no card, but a recorded reason and an audit row:
/// the skip is observable, never a silent nothing.
#[tokio::test]
async fn scenario_07_a_call_without_email_records_a_reason() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;
    let lead = create_lead(&app, &key, &fresh_phone()).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();

    let body = finish_call(&app, &key, &lead, "call-s07").await;
    assert_eq!(body["order_card"]["state"], "skipped");
    assert_eq!(body["order_card"]["reason"], "missing_email");

    let (outcome, reason) = card_audit(&pg, &lead).await.expect("audit row");
    assert_eq!(outcome, "denied");
    assert_eq!(reason.as_deref(), Some("missing_email"));
}

/// §8.8 — a half-collected lead never produces a half-addressed card:
/// the blocking gaps are the reason, and they are listed.
#[tokio::test]
async fn scenario_08_an_incomplete_call_never_queues_a_card() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;
    let lead = create_lead(&app, &key, &fresh_phone()).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();

    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/v1/voice/leads/{lead}"),
        Some(&key),
        Some(json!({ "contact_email": " Buyer@Acme.TEST " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch email: {body}");

    let body = finish_call(&app, &key, &lead, "call-s08").await;
    assert_eq!(body["order_card"]["state"], "skipped");
    assert_eq!(body["order_card"]["reason"], "incomplete_requirements");
    assert!(!body["order_card"]["missing_fields"]
        .as_array()
        .expect("missing_fields")
        .is_empty());

    let (outcome, reason) = card_audit(&pg, &lead).await.expect("audit row");
    assert_eq!(outcome, "denied");
    assert_eq!(reason.as_deref(), Some("incomplete_requirements"));
}

/// §8.9 — the complete call: one card, queued once, replayed finishes
/// collapse onto it, audit says success.
#[tokio::test]
async fn scenario_09_one_complete_call_one_order_card() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;
    let lead = create_lead(&app, &key, &fresh_phone()).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();

    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/v1/voice/leads/{lead}"),
        Some(&key),
        Some(json!({ "contact_email": "buyer@acme.test" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch email: {body}");
    fill_gaps(&app, &key, &lead).await;

    let body = finish_call(&app, &key, &lead, "call-s09a").await;
    assert_eq!(body["order_card"]["state"], "queued", "{body}");
    let outbox_id = body["order_card"]["outbox_id"]
        .as_str()
        .expect("outbox_id")
        .parse::<Uuid>()
        .expect("uuid");

    let row: (String, String, String, String) = sqlx::query_as(
        "SELECT message_type, recipient, status, idempotency_key \
         FROM mailbox_outbox WHERE id = $1",
    )
    .bind(outbox_id)
    .fetch_one(&pg)
    .await
    .expect("outbox row");
    assert_eq!(row.0, "order_card");
    assert_eq!(row.1, "buyer@acme.test");
    assert_eq!(row.2, "queued");
    assert!(row.3.starts_with(&format!("order_card:{lead}:")));

    let (outcome, reason) = card_audit(&pg, &lead).await.expect("audit row");
    assert_eq!(outcome, "success");
    assert_eq!(reason, None);

    let body = finish_call(&app, &key, &lead, "call-s09b").await;
    assert_eq!(body["order_card"]["state"], "already_queued");

    let lead_uuid = lead.parse::<Uuid>().expect("uuid");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailbox_outbox \
         WHERE lead_id = $1 AND message_type = 'order_card'",
    )
    .bind(lead_uuid)
    .fetch_one(&pg)
    .await
    .expect("count");
    assert_eq!(count, 1, "one card per lead, ever");
}

/// §8.10 — nothing about a call lives in process memory: a restart (same
/// database, brand-new server state) resumes the lead, the facts and the
/// asked questions exactly.
#[tokio::test]
async fn scenario_10_call_state_lives_in_postgres_not_in_memory() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;
    let phone = fresh_phone();

    let before_app = api(pg.clone()).await;
    let lead = create_lead(&before_app, &key, &phone).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();
    let (status, body) = call(
        &before_app,
        "PUT",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        Some(json!({ "requirements": [{ "field": "goods_name", "value": "телескопы" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "save: {body}");
    append_message(&before_app, &key, &lead, "outbound", "Какой груз?").await;

    // "Restart": a completely fresh router and state over the same database.
    let after_app = api(pg.clone()).await;
    let resumed = create_lead(&after_app, &key, &phone).await;
    assert_eq!(resumed["created"], false, "the lead survives the restart");
    assert_eq!(resumed["lead_id"], lead);

    let (_, requirements) = call(
        &after_app,
        "GET",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&key),
        None,
    )
    .await;
    let goods = requirements["requirements"]
        .as_array()
        .expect("requirements")
        .iter()
        .find(|item| item["field"] == "goods_name")
        .expect("goods_name row");
    assert_eq!(goods["value"], "телескопы");
    let asked = requirements["asked_questions"]
        .as_array()
        .expect("asked_questions");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["question"], "Какой груз?");
}

// ===========================================================================
// B. The email pipeline (mock LLM, mock mail)
// ===========================================================================

/// §8.11 — an inbound email becomes a lead and a reply that actually
/// leaves through the outbox worker (mock SMTP, no real send).
#[tokio::test]
async fn scenario_11_inbound_mail_becomes_a_lead_and_a_reply() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    clear_pending(&pg).await;
    let llm = Arc::new(MockLlmProvider::new(&llm_settings()));
    stub_pipeline(&llm, &qualification_partial(), &reply_asking());
    let orch = build_orchestrator(pg.clone(), llm as Arc<dyn LlmProvider>, auto_config());

    let buyer = format!("buyer-{}@example.com", uuid::Uuid::new_v4());
    let id = ingest(
        &pg,
        &email(
            &buyer,
            &format!("Доставка станков {}", uuid::Uuid::new_v4()),
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    let stored = email_repo::get(&pg, id).await.expect("get");
    assert_eq!(stored.status, EmailStatus::Processed);
    let lead_id = stored.lead_id.expect("a lead must open");
    assert_eq!(
        outbox_rows(&pg, "lead_id = $1", lead_id).await,
        vec![(
            "customer_reply".to_string(),
            buyer.clone(),
            "queued".to_string()
        )],
        "auto mode must queue the reply addressed to the sender"
    );

    let mock = Arc::new(MockMailProvider::with_mode(MailMode::Work));
    let writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pg, writer, auto_config()).await;

    assert_eq!(
        sent_to(&mock, &buyer).await.len(),
        1,
        "exactly one delivery"
    );
    assert_eq!(
        outbox_rows(&pg, "lead_id = $1", lead_id).await,
        vec![(
            "customer_reply".to_string(),
            buyer.clone(),
            "sent".to_string()
        )]
    );
}

/// §8.12 — review mode prepares exactly the same reply but holds it: the
/// draft exists, the outbox does not.
#[tokio::test]
async fn scenario_12_review_mode_holds_the_reply_for_a_human() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let llm = Arc::new(MockLlmProvider::new(&llm_settings()));
    stub_pipeline(&llm, &qualification_partial(), &reply_asking());
    let orch = build_orchestrator(pg.clone(), llm as Arc<dyn LlmProvider>, review_config());

    let buyer = format!("buyer-{}@example.com", uuid::Uuid::new_v4());
    let id = ingest(
        &pg,
        &email(
            &buyer,
            &format!("Доставка станков {}", uuid::Uuid::new_v4()),
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    let lead_id = email_repo::get(&pg, id)
        .await
        .expect("get")
        .lead_id
        .expect("a lead must open");
    assert_eq!(drafts_for(&pg, lead_id).await, 1, "the reply is prepared");
    assert!(
        outbox_rows(&pg, "lead_id = $1", lead_id).await.is_empty(),
        "review mode must not let the same reply out"
    );
}

/// §8.13 — spam dies before the outbox: quarantined, zero outbound rows.
#[tokio::test]
async fn scenario_13_spam_never_reaches_the_outbox() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let llm = Arc::new(MockLlmProvider::new(&llm_settings()));
    llm.stub_json("Скидка", &replies::spam());
    let orch = build_orchestrator(pg.clone(), llm as Arc<dyn LlmProvider>, auto_config());

    let id = ingest(
        &pg,
        &email(
            "spammer@example.com",
            &format!("Скидка 70% {}", uuid::Uuid::new_v4()),
            "Купите наши услуги",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    let stored = email_repo::get(&pg, id).await.expect("get");
    assert_eq!(stored.status, EmailStatus::Quarantined);
    let rows =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mailbox_outbox WHERE email_id = $1")
            .bind(id)
            .fetch_one(&pg)
            .await
            .expect("count outbox");
    assert_eq!(rows, 0, "a convicted message must never produce a row");
}

/// §8.14 — idempotency: the same email processed twice queues one reply,
/// not two.
#[tokio::test]
async fn scenario_14_reprocessing_never_sends_a_second_reply() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    clear_pending(&pg).await;
    let llm = Arc::new(MockLlmProvider::new(&llm_settings()));
    stub_pipeline(&llm, &qualification_partial(), &reply_asking());
    let orch = build_orchestrator(pg.clone(), llm as Arc<dyn LlmProvider>, auto_config());

    let buyer = format!("buyer-{}@example.com", uuid::Uuid::new_v4());
    let id = ingest(
        &pg,
        &email(
            &buyer,
            &format!("Доставка станков {}", uuid::Uuid::new_v4()),
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;

    orch.process_email(id).await.expect("first process");
    orch.process_email(id).await.expect("second process");

    let rows =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mailbox_outbox WHERE email_id = $1")
            .bind(id)
            .fetch_one(&pg)
            .await
            .expect("count outbox");
    assert_eq!(rows, 1, "a replayed message must not queue a second reply");
}

/// §8.15 — the question suppression lives on the server: the model is
/// stubbed to repeat itself verbatim and the second draft is still not
/// asked twice, on the same lead, with the answer recorded.
#[tokio::test]
async fn scenario_15_a_repeated_question_is_dropped_by_the_server() {
    const QUESTION: &str = "Уточните, пожалуйста, вес и объём груза?";
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let llm = Arc::new(MockLlmProvider::new(&llm_settings()));
    let orch = build_orchestrator(
        pg.clone(),
        llm.clone() as Arc<dyn LlmProvider>,
        base_config(),
    );

    let nonce = uuid::Uuid::new_v4();
    let from = format!("buyer-{nonce}@example.com");
    let subject = format!("Доставка из Китая {nonce}");

    // Turn one: the model asks for the weight.
    stub_pipeline(
        &llm,
        &qualification_partial(),
        &json!({
            "subject": "Re: Доставка из Китая",
            "body": "Здравствуйте! Уточните, пожалуйста, вес и объём груза?",
            "disposition": "draft",
            "questions": [QUESTION],
            "handoff_requested": false,
            "handoff_reason": null,
            "confidence": 0.9,
            "rationale": "параметры груза ещё неизвестны"
        }),
    );
    let first = ingest(
        &pg,
        &email(
            &from,
            &subject,
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;
    orch.process_email(first).await.expect("turn one");
    let lead_id = email_repo::get(&pg, first)
        .await
        .expect("get")
        .lead_id
        .expect("a lead must open");
    assert_eq!(drafts_for(&pg, lead_id).await, 1, "turn one asks");

    // Turn two: the customer answers, the model repeats the same question.
    llm.reset();
    stub_pipeline(
        &llm,
        &json!({
            "lead_id": null,
            "first_email_id": null,
            "company_name": "ООО ОптОрг",
            "contact_name": "Иван Петров",
            "contact_phone": "+7 (495) 123-45-67",
            "summary": "Перевозка оборудования из Китая в Россию",
            "scope": "transport",
            "needs_expert": false,
            "questions": [],
            "confidence": 0.9,
            "regulated_topics": [],
            "requirements": [
                { "field": "goods_weight", "value": "12000", "state": "known",
                  "unit": "kg", "confidence": 0.95, "evidence": "12000 кг" }
            ]
        }),
        &json!({
            "subject": "Re: Доставка из Китая",
            "body": "Спасибо. Уточните, пожалуйста, вес и объём груза?",
            "disposition": "draft",
            "questions": [QUESTION],
            "handoff_requested": false,
            "handoff_reason": null,
            "confidence": 0.9,
            "rationale": "модель не проверила список заданных вопросов"
        }),
    );
    let second = ingest(
        &pg,
        &email(
            &from,
            &format!("Re: {subject}"),
            "Вес груза 12000 кг, объём 30 м3. Когда сможете забрать?",
        ),
    )
    .await;
    orch.process_email(second).await.expect("turn two");

    let second_email = email_repo::get(&pg, second).await.expect("get");
    assert_eq!(
        second_email.lead_id,
        Some(lead_id),
        "a reply in the same thread must not open a second lead"
    );
    let weight = requirement_repo::get_field(&pg, lead_id, RequirementField::GoodsWeight)
        .await
        .expect("read goods_weight")
        .and_then(|requirement| requirement.value);
    assert_eq!(
        weight.as_deref(),
        Some("12000"),
        "the answer reaches the lead"
    );
    assert_eq!(
        drafts_for(&pg, lead_id).await,
        1,
        "a question already asked is not asked again"
    );
}

// ===========================================================================
// C. The seam between the two applications, and the safety gates
// ===========================================================================

/// §8.16 — the same human on the phone and on email: two channels, two
/// leads, no cross-contamination of keys or contact data.
#[tokio::test]
async fn scenario_16_voice_and_email_never_share_a_lead() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;

    let app = api(pg.clone()).await;
    let nonce = uuid::Uuid::new_v4();
    let address = format!("buyer-{nonce}@example.com");
    let voice_lead = create_lead(&app, &key, &fresh_phone()).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();
    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/v1/voice/leads/{voice_lead}"),
        Some(&key),
        Some(json!({ "contact_email": address })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch email: {body}");

    // The same address writes to us by mail.
    let llm = Arc::new(MockLlmProvider::new(&llm_settings()));
    stub_pipeline(&llm, &qualification_partial(), &reply_asking());
    let orch = build_orchestrator(pg.clone(), llm as Arc<dyn LlmProvider>, auto_config());
    let id = ingest(
        &pg,
        &email(
            &address,
            &format!("Перевозка {}", uuid::Uuid::new_v4()),
            "Нужно перевезти груз из Москвы в Казань",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");
    let email_lead = email_repo::get(&pg, id)
        .await
        .expect("get")
        .lead_id
        .expect("the email must open a lead");

    let voice_uuid = voice_lead.parse::<Uuid>().expect("uuid");
    assert_ne!(
        voice_uuid, email_lead,
        "the channels must not merge their leads"
    );
    let (conversation_key, contact_email): (String, Option<String>) =
        sqlx::query_as("SELECT conversation_key, contact_email FROM leads WHERE id = $1")
            .bind(voice_uuid)
            .fetch_one(&pg)
            .await
            .expect("voice lead row");
    assert!(
        conversation_key.starts_with("voice:"),
        "the voice key must stay a voice key: {conversation_key}"
    );
    assert_eq!(contact_email.as_deref(), Some(address.as_str()));
}

/// §8.17 — the full seam: a finished call queues a card into the shared
/// outbox, and the send worker delivers it through the mail transport
/// (mock SMTP) exactly like any other outbound message.
#[tokio::test]
async fn scenario_17_the_order_card_leaves_through_the_shared_outbox() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    clear_pending(&pg).await;
    let key = mint(&pg, "scenario-operator", api_key_repo::Role::Operator).await;

    let app = api(pg.clone()).await;
    let buyer = format!("buyer-{}@example.com", uuid::Uuid::new_v4());
    let lead = create_lead(&app, &key, &fresh_phone()).await["lead_id"]
        .as_str()
        .expect("lead_id")
        .to_string();
    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/v1/voice/leads/{lead}"),
        Some(&key),
        Some(json!({ "contact_email": buyer })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch email: {body}");
    fill_gaps(&app, &key, &lead).await;

    let body = finish_call(&app, &key, &lead, "call-s17").await;
    assert_eq!(body["order_card"]["state"], "queued", "{body}");

    let mock = Arc::new(MockMailProvider::with_mode(MailMode::Work));
    let writer: Arc<dyn MaybeWritable> = mock.clone();
    run_worker(&pg, writer, auto_config()).await;

    assert_eq!(
        outbox_rows(
            &pg,
            "lead_id = $1 AND message_type = 'order_card'",
            lead.parse::<Uuid>().expect("uuid")
        )
        .await,
        vec![("order_card".to_string(), buyer.clone(), "sent".to_string())],
        "the card is delivered through the same outbox as every reply"
    );
    let delivered = sent_to(&mock, &buyer).await;
    assert_eq!(delivered.len(), 1, "exactly one card left the building");
    assert!(
        delivered[0].0.starts_with("Карточка заказа"),
        "customer-facing subject, not an internal one: {}",
        delivered[0].0
    );
    assert!(delivered[0].1 > 0, "the card has a body");
}

/// §8.18 — the voice API is closed without a key, while liveness stays
/// open for Docker and the load balancer.
#[tokio::test]
async fn scenario_18_the_voice_api_demands_a_key() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;

    let (status, _) = call(
        &app,
        "GET",
        &format!("/api/v1/voice/leads/{}/requirements", uuid::Uuid::new_v4()),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/voice/leads",
        None,
        Some(json!({ "caller_number": "79001234567" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = call(&app, "GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK, "/health stays public");
}

/// §8.19 — a viewer may read the call state but may not drive it: the
/// role gate holds on the mutating voice endpoints.
#[tokio::test]
async fn scenario_19_a_viewer_cannot_drive_a_call() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };
    let app = api(pg.clone()).await;
    let viewer = mint(&pg, "scenario-viewer", api_key_repo::Role::Viewer).await;

    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/voice/leads",
        Some(&viewer),
        Some(json!({ "caller_number": fresh_phone() })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "viewer creates nothing: {body}"
    );

    let lead = uuid::Uuid::new_v4();
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        Some(&viewer),
        Some(json!({ "requirements": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/voice/calls/finish",
        Some(&viewer),
        Some(json!({ "lead_id": lead, "call_id": "x", "outcome": "completed" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// §8.20 — liveness and readiness say different things: the process is
/// up either way, readiness only while the database answers — which is
/// what the container healthcheck probes.
#[tokio::test]
async fn scenario_20_readiness_tracks_the_database() {
    let _guard = SERIAL.lock().await;
    let Some(pg) = test_pool().await else { return };

    let live = api(pg.clone()).await;
    let (status, body) = call(&live, "GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["database"], "connected");
    let (status, _) = call(&live, "GET", "/ready", None, None).await;
    assert_eq!(status, StatusCode::OK);

    // The pool points where nothing listens: readiness must fail loudly
    // while liveness keeps reporting the process is serving.
    let dead = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://mca:mca@127.0.0.1:55432/mca_mail_ci9")
        .expect("lazy pool");
    let dead_app = api::build_router(&ApiSettings::default(), Arc::new(ApiState::new(dead)));
    let (status, body) = call(&dead_app, "GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK, "liveness does not depend on the DB");
    assert_eq!(body["database"], "disconnected");
    let (status, _) = call(&dead_app, "GET", "/ready", None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
