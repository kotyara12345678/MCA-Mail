//! Stage G: the production auto-responder.
//!
//! Five claims have to hold together before this may touch a real customer:
//! a reply the pipeline produced actually leaves the building, review mode
//! still holds the very same reply, a qualified lead produces one manager
//! card to every configured address, spam never reaches the outbox at all,
//! and — the gate the whole dialogue hangs on — only order traffic is ever
//! answered: a partner pitch, a promo or an analytics dump that still opens
//! a lead gets no draft, no question and no send. The sixth is idempotency:
//! one inbound email, one queued reply.

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

fn classification(category: &str, confidence: f32) -> serde_json::Value {
    serde_json::json!({
        "category": category,
        "confidence": confidence,
        "explanation": "auto-reply contract fixture",
        "requires_human": false,
        "suggested_action": "qualify"
    })
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

/// The dialogue belongs to orders. A partner pitch that classification
/// still turns into a lead (partners may open one) must produce no draft,
/// no question and no outbox row — the red flag is the gate, and only
/// order traffic earns it. The order that follows in the same test proves
/// the very same pipeline still answers when the flag is up.
#[tokio::test]
async fn only_order_traffic_earns_an_automatic_reply() {
    let _guard = SERIAL.lock().await;
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &classification("partner", 0.9),
    );
    llm.stub_json("lead qualification agent", &qualification_partial());
    llm.stub_json("customer manager at MCA Logistics", &reply_asking());
    let orch = build_orchestrator(
        auto_config(),
        pool.clone(),
        Arc::clone(&llm) as Arc<dyn LlmProvider>,
    );

    let partner = format!("partner-{}@example.com", uuid::Uuid::new_v4());
    let pitch = ingest(
        &pool,
        &email(
            &partner,
            &format!("Совместный вебинар {}", uuid::Uuid::new_v4()),
            "Предлагаем провести совместный вебинар для ваших клиентов.",
        ),
    )
    .await;
    orch.process_email(pitch).await.expect("process pitch");

    let pitch_lead = email_repo::get(&pool, pitch)
        .await
        .expect("get")
        .lead_id
        .expect("a partner email may still open a lead");
    assert_eq!(
        drafts_for(&pool, pitch_lead).await,
        0,
        "a non-order lead must receive no draft"
    );
    assert_eq!(
        outbox_for_lead(&pool, pitch_lead).await,
        Vec::<(String, String, String)>::new(),
        "a non-order lead must receive nothing"
    );

    // The same pipeline, an order this time: the flag goes up and the
    // reply the model prepared reaches the outbox.
    llm.reset();
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &classification("transport_request", 0.9),
    );
    llm.stub_json("lead qualification agent", &qualification_partial());
    llm.stub_json("customer manager at MCA Logistics", &reply_asking());
    let buyer = format!("buyer-{}@example.com", uuid::Uuid::new_v4());
    let order = ingest(
        &pool,
        &email(
            &buyer,
            &format!("Доставка станков {}", uuid::Uuid::new_v4()),
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;
    orch.process_email(order).await.expect("process order");

    let order_lead = email_repo::get(&pool, order)
        .await
        .expect("get")
        .lead_id
        .expect("an order opens a lead");
    assert_eq!(
        outbox_for_lead(&pool, order_lead).await,
        vec![(
            "customer_reply".to_string(),
            buyer.clone(),
            "queued".to_string()
        )],
        "an order is answered"
    );
}

/// A customer who asks for a call is answered in the same turn that opens the
/// manager's handoff ticket — and the ticket leaves the dialogue running: the
/// lead stays unlocked, so every message that follows still gets an answer.
#[tokio::test]
async fn a_callback_request_is_answered_without_locking_the_lead() {
    let _guard = SERIAL.lock().await;
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    let mut qualification = qualification_partial();
    qualification["needs_expert"] = serde_json::json!(true);
    qualification["contact_phone"] = serde_json::json!("+7 900 123-45-67");
    stub_pipeline(&llm, &qualification, &reply_asking());
    llm.stub_json(
        "handoff agent for MCA Logistics",
        &serde_json::json!({
            "reason": "phone call requested",
            "priority": 50,
            "cargo_summary": "станки",
            "route_summary": "Шэньчжэнь — Москва",
            "requested_service": "transport",
            "missing_information": ["вес", "объём"],
            "open_questions": [],
            "conversation_digest": "клиент просит позвонить",
            "checks_performed": [],
            "unresolved_topics": [],
            "original_request": "перевезти станки, позвоните мне"
        }),
    );
    let orch = build_orchestrator(auto_config(), pool.clone(), llm as Arc<dyn LlmProvider>);

    let buyer = format!("caller-{}@example.com", uuid::Uuid::new_v4());
    let id = ingest(
        &pool,
        &email(
            &buyer,
            &format!("Звонок и станки {}", uuid::Uuid::new_v4()),
            "Нужно перевезти станки из Шэньчжэня в Москву, позвоните мне",
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
        outbox_for_lead(&pool, lead_id).await,
        vec![(
            "customer_reply".to_string(),
            buyer.clone(),
            "queued".to_string()
        )],
        "the same turn's reply must reach the outbox"
    );

    let locked: bool = sqlx::query_scalar("SELECT automation_locked FROM leads WHERE id = $1")
        .bind(lead_id)
        .fetch_one(&pool)
        .await
        .expect("lead");
    assert!(
        !locked,
        "the callback ticket must not stop the automatic dialogue"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM leads WHERE id = $1")
        .bind(lead_id)
        .fetch_one(&pool)
        .await
        .expect("lead status");
    assert_ne!(
        status, "handed_off",
        "only a blocking reason flips the lead's status"
    );
    let handoffs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM human_handoffs WHERE lead_id = $1")
            .bind(lead_id)
            .fetch_one(&pool)
            .await
            .expect("handoffs");
    assert_eq!(handoffs, 1, "the manager gets exactly one open handoff");
}

/// A price request may flag a handoff but must never end the conversation:
/// turn one answers with the gaps still open and opens no ticket, turn two
/// closes the gaps — only then does the ticket open and the cards go out,
/// with the lead still unlocked throughout.
#[tokio::test]
async fn a_pricing_handoff_keeps_the_dialogue_alive_until_the_card() {
    let _guard = SERIAL.lock().await;
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));

    // Turn one: the model — against its instructions — flags a pricing
    // handoff while the dialogue still has gaps. The server must decline the
    // ticket and answer anyway.
    let pricing_reply = serde_json::json!({
        "subject": "Re: Доставка станков",
        "body": "Здравствуйте! Уточните, пожалуйста, вес и объём груза — по ним посчитаем стоимость.",
        "disposition": "send",
        "questions": ["Уточните, пожалуйста, вес и объём груза?"],
        "handoff_requested": true,
        "handoff_reason": "pricing_or_legal",
        "confidence": 0.9,
        "rationale": "клиент спрашивает о цене"
    });
    stub_pipeline(&llm, &qualification_partial(), &pricing_reply);
    let orch = build_orchestrator(
        auto_config(),
        pool.clone(),
        llm.clone() as Arc<dyn LlmProvider>,
    );

    let buyer = format!("pricer-{}@example.com", uuid::Uuid::new_v4());
    let subject = format!("Стоимость и станки {}", uuid::Uuid::new_v4());
    let id_one = ingest(
        &pool,
        &email(
            &buyer,
            &subject,
            "Сколько стоит перевезти станки из Шэньчжэня в Москву?",
        ),
    )
    .await;
    orch.process_email(id_one).await.expect("turn one");

    let lead_id = email_repo::get(&pool, id_one)
        .await
        .expect("get")
        .lead_id
        .expect("a lead must open");
    assert_eq!(
        outbox_for_lead(&pool, lead_id).await,
        vec![(
            "customer_reply".to_string(),
            buyer.clone(),
            "queued".to_string()
        )],
        "turn one must answer the price request with the questions it asks"
    );
    assert!(
        !lead_locked(&pool, lead_id).await,
        "a pricing flag must not lock the lead"
    );
    assert_eq!(
        handoff_count(&pool, lead_id).await,
        0,
        "while the dialogue has gaps, a pricing flag opens no ticket"
    );

    // Turn two: the customer supplies everything, the dialogue completes —
    // and the model flags the handoff it was never supposed to flag again.
    let closing_reply = serde_json::json!({
        "subject": "Re: Доставка станков",
        "body": "Здравствуйте! Информация полна, менеджер свяжется с вами.",
        "disposition": "send",
        "questions": [],
        "handoff_requested": true,
        "handoff_reason": "pricing_or_legal",
        "confidence": 0.9,
        "rationale": "данные собраны, клиент ждёт расчёт"
    });
    llm.stub_json("lead qualification agent", &qualification_complete());
    llm.stub_json("customer manager at MCA Logistics", &closing_reply);
    llm.stub_json(
        "handoff agent for MCA Logistics",
        &serde_json::json!({
            "reason": "all details collected",
            "priority": 50,
            "cargo_summary": "станки",
            "route_summary": "Шэньчжэнь — Москва",
            "requested_service": "transport",
            "missing_information": [],
            "open_questions": [],
            "conversation_digest": "диалог завершён, клиенту нужен расчёт",
            "checks_performed": [],
            "unresolved_topics": [],
            "original_request": "перевезти станки из Шэньчжэня в Москву"
        }),
    );
    let id_two = ingest(
        &pool,
        &email(
            &buyer,
            &subject,
            "Вес 12000 кг, объём 30 м3, 2 шт, авто, до конца месяца.",
        ),
    )
    .await;
    orch.process_email(id_two).await.expect("turn two");

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
        "the completed dialogue answers, queues a card to every manager, and opens the ticket"
    );
    assert_eq!(
        handoff_count(&pool, lead_id).await,
        1,
        "the completed dialogue opens exactly one ticket for the manager"
    );
    assert!(
        !lead_locked(&pool, lead_id).await,
        "even a completed dialogue leaves the lead unlocked for the next message"
    );
}

async fn lead_locked(pool: &sqlx::PgPool, lead_id: LeadId) -> bool {
    sqlx::query_scalar("SELECT automation_locked FROM leads WHERE id = $1")
        .bind(lead_id)
        .fetch_one(pool)
        .await
        .expect("lead")
}

async fn handoff_count(pool: &sqlx::PgPool, lead_id: LeadId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM human_handoffs WHERE lead_id = $1")
        .bind(lead_id)
        .fetch_one(pool)
        .await
        .expect("handoffs")
}
