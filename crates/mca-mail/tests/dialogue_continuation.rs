//! Stage C: a second email continues the dialogue instead of starting one.
//!
//! Three things have to hold at once for the pilot to be safe: the customer's
//! answer lands on the *same* lead, the facts it carries are written to that
//! lead, and the question the first reply already put is not put again. The
//! third one is the reason the suppression lives in the server rather than in
//! the prompt: the model is stubbed here to repeat itself verbatim.

use std::sync::Arc;

use mca_mail::config::AppConfig;
use mca_mail::domain::{EmailAddress, EmailStatus, InboundMessage, RequirementField};
use mca_mail::llm::mock::MockLlmProvider;
use mca_mail::llm::LlmProvider;
use mca_mail::orchestration::{Orchestrator, OrchestratorBuilder};
use mca_mail::persistence::{email_repo, requirement_repo, thread_repo};
use mca_mail::tools::ToolRegistry;
use mca_mail_testkit::replies;

/// What turn one asks for, and what turn two therefore must not ask for again.
const QUESTION: &str = "Уточните, пожалуйста, вес и объём груза?";

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

fn build_orchestrator(pool: sqlx::PgPool, llm: Arc<dyn LlmProvider>) -> Arc<Orchestrator> {
    Arc::new(
        OrchestratorBuilder::new()
            .config(test_config())
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

/// Turn one's reply: a real draft, carrying the question.
fn turn_one_reply() -> serde_json::Value {
    serde_json::json!({
        "subject": "Re: Доставка из Китая",
        "body": "Здравствуйте! Спасибо за запрос. Уточните, пожалуйста, вес и объём груза?",
        "disposition": "draft",
        "questions": [QUESTION],
        "handoff_requested": false,
        "handoff_reason": null,
        "confidence": 0.9,
        "rationale": "параметры груза ещё неизвестны"
    })
}

/// Turn two's reply: the model asks the *same* question, on purpose.
fn turn_two_reply() -> serde_json::Value {
    serde_json::json!({
        "subject": "Re: Доставка из Китая",
        "body": "Спасибо. Уточните, пожалуйста, вес и объём груза?",
        "disposition": "draft",
        "questions": [QUESTION],
        "handoff_requested": false,
        "handoff_reason": null,
        "confidence": 0.9,
        "rationale": "модель не проверила список заданных вопросов"
    })
}

/// Turn one extracts nothing: the customer has not said it yet.
fn turn_one_qualification() -> serde_json::Value {
    replies::qualification_transport()
}

/// Turn two carries the numbers the customer just gave, with the words they
/// came from attached.
fn turn_two_qualification() -> serde_json::Value {
    serde_json::json!({
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
            {
                "field": "goods_weight",
                "value": "12000",
                "state": "known",
                "unit": "kg",
                "confidence": 0.95,
                "evidence": "Вес груза 12000 кг"
            },
            {
                "field": "goods_volume",
                "value": "30",
                "state": "known",
                "unit": "м3",
                "confidence": 0.95,
                "evidence": "объём 30 м3"
            }
        ]
    })
}

/// Register the four agents for one turn. Stubs are keyed on the agent's own
/// system prompt, so the two turns must be stubbed separately: reset between
/// them rather than trying to tell the turns apart by content.
fn stub_turn_one(llm: &MockLlmProvider) {
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &replies::classification_lead(),
    );
    llm.stub_json("lead qualification agent", &turn_one_qualification());
    llm.stub_json("customer manager at MCA Logistics", &turn_one_reply());
}

fn stub_turn_two(llm: &MockLlmProvider) {
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &replies::classification_lead(),
    );
    llm.stub_json("lead qualification agent", &turn_two_qualification());
    llm.stub_json("customer manager at MCA Logistics", &turn_two_reply());
}

async fn drafts_for(pool: &sqlx::PgPool, lead_id: mca_mail::domain::LeadId) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM email_drafts WHERE lead_id = $1")
        .bind(lead_id)
        .fetch_one(pool)
        .await
        .expect("count drafts")
}

async fn weight(pool: &sqlx::PgPool, lead_id: mca_mail::domain::LeadId) -> Option<String> {
    requirement_repo::get_field(pool, lead_id, RequirementField::GoodsWeight)
        .await
        .expect("read goods_weight")
        .and_then(|requirement| requirement.value)
}

#[tokio::test]
async fn a_second_email_continues_one_dialogue() {
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn_one(&llm);
    let orch = build_orchestrator(pool.clone(), llm.clone() as Arc<dyn LlmProvider>);

    // The thread key is the sender plus the normalised subject, and the
    // database outlives a run: without a per-run sender the lead opened here
    // would be the one an earlier run (or another test) already filled in.
    let nonce = uuid::Uuid::new_v4();
    let from = format!("buyer-{nonce}@example.com");
    let subject = format!("Доставка из Китая {nonce}");

    // --- Turn one: the customer asks, we ask what is missing. ---------------
    let first = ingest(
        &pool,
        &email(
            &from,
            &subject,
            "Нужно перевезти станки из Шэньчжэня в Москву",
        ),
    )
    .await;
    orch.process_email(first).await.expect("turn one");

    let first_email = email_repo::get(&pool, first).await.expect("get");
    let lead_id = first_email
        .lead_id
        .expect("the first email must open a lead");
    assert_eq!(first_email.status, EmailStatus::Processed);

    let asked_before = drafts_for(&pool, lead_id).await;
    assert_eq!(asked_before, 1, "turn one must prepare exactly one draft");
    assert_eq!(
        weight(&pool, lead_id).await,
        None,
        "the weight is still missing, which is why it was asked for"
    );

    // --- Turn two: the customer answers, the model repeats its question. ----
    llm.reset();
    stub_turn_two(&llm);

    let second = ingest(
        &pool,
        &email(
            &from,
            &format!("Re: {subject}"),
            "Вес груза 12000 кг, объём 30 м3. Когда сможете забрать?",
        ),
    )
    .await;
    orch.process_email(second).await.expect("turn two");

    let second_email = email_repo::get(&pool, second).await.expect("get");
    assert_eq!(
        second_email.lead_id,
        Some(lead_id),
        "a reply in the same thread must not open a second lead"
    );
    assert_eq!(second_email.status, EmailStatus::Processed);

    assert_eq!(
        weight(&pool, lead_id).await,
        Some("12000".to_string()),
        "the customer's answer must reach the lead's requirements"
    );

    assert_eq!(
        drafts_for(&pool, lead_id).await,
        asked_before,
        "a question already asked is not asked a second time"
    );
}
