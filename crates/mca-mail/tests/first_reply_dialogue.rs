//! Stage H: the first reply asks for the whole missing set in one message.
//!
//! A customer should never have to answer one question per email. This file
//! pins the server-side half of that contract: the reply agent is handed
//! *every* field the customer has not given yet — not only the handful that
//! block a quote — the client's own words are visible as known facts so
//! nothing is asked twice, and the whole ask still leaves as a single draft.

use std::sync::Arc;

use mca_mail::config::AppConfig;
use mca_mail::domain::{EmailAddress, EmailStatus, InboundMessage, RequirementField};
use mca_mail::llm::mock::MockLlmProvider;
use mca_mail::llm::LlmProvider;
use mca_mail::orchestration::{Orchestrator, OrchestratorBuilder};
use mca_mail::persistence::{draft_repo, email_repo, requirement_repo, thread_repo};
use mca_mail::tools::ToolRegistry;
use mca_mail_testkit::replies;

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

/// Only what the first sentence of the customer actually says: the goods, the
/// country it ships from and the city it ends in. Everything else has to be
/// asked for.
fn turn_one_qualification() -> serde_json::Value {
    serde_json::json!({
        "lead_id": null,
        "first_email_id": null,
        "company_name": null,
        "contact_name": null,
        "contact_phone": null,
        "summary": "Доставка оборудования из Китая в Москву",
        "scope": "full_import",
        "needs_expert": false,
        "questions": [],
        "confidence": 0.9,
        "regulated_topics": [],
        "requirements": [
            req("goods_name", "оборудование", None),
            req("origin_country", "Китай", None),
            req("destination_city", "Москва", None)
        ]
    })
}

/// The whole ask, as one numbered list inside a single message.
fn turn_one_reply() -> serde_json::Value {
    serde_json::json!({
        "subject": "Re: Доставка оборудования",
        "body": "Здравствуйте! Спасибо за запрос. Чтобы подготовить предложение, сообщите, пожалуйста:\n\
                 1. Что за оборудование и в каком количестве?\n\
                 2. Какой вес груза?\n\
                 3. Какой объём?\n\
                 4. Из какого города забирать?\n\
                 5. В какую страну доставлять?\n\
                 6. Какой способ доставки: авто, море или ж/д?\n\
                 7. Желаемый срок доставки?\n\
                 8. Какова страховая стоимость?\n\
                 9. Нужна ли закупка у поставщика?\n\
                 10. Нужна ли таможенное оформление?\n\
                 11. Название вашей компании?\n\
                 12. ИНН?\n\
                 13. Контактное лицо?\n\
                 14. Телефон?\n\
                 15. Особые требования?",
        "disposition": "draft",
        "questions": [
            "Что за оборудование и в каком количестве?",
            "Какой вес груза?",
            "Какой объём?",
            "Из какого города забирать?",
            "В какую страну доставлять?",
            "Какой способ доставки: авто, море или ж/д?",
            "Желаемый срок доставки?",
            "Какова страховая стоимость?",
            "Нужна ли закупка у поставщика?",
            "Нужна ли таможенное оформление?",
            "Название вашей компании?",
            "ИНН?",
            "Контактное лицо?",
            "Телефон?",
            "Особые требования?"
        ],
        "handoff_requested": false,
        "handoff_reason": null,
        "confidence": 0.9,
        "rationale": "клиент назвал только товар и маршрут"
    })
}

/// The customer answers the whole list in one go.
fn turn_two_qualification() -> serde_json::Value {
    serde_json::json!({
        "lead_id": null,
        "first_email_id": null,
        "company_name": "ООО Рога и Копыта",
        "contact_name": "Иван Иванов",
        "contact_phone": "+7 495 123-45-67",
        "summary": "Доставка оборудования из Китая в Москву, вес и объём известны",
        "scope": "full_import",
        "needs_expert": false,
        "questions": [],
        "confidence": 0.9,
        "regulated_topics": [],
        "requirements": [
            req("goods_weight", "12000", Some("kg")),
            req("goods_volume", "30", Some("м3")),
            req("company_name", "ООО Рога и Копыта", None),
            req("company_inn", "7701234567", None),
            req("contact_name", "Иван Иванов", None),
            req("contact_phone", "+7 495 123-45-67", None)
        ]
    })
}

/// Turn two's reply: still one message, and the remaining items are worded
/// afresh so the server sees them as questions it has not put yet.
fn turn_two_reply() -> serde_json::Value {
    serde_json::json!({
        "subject": "Re: Доставка оборудования",
        "body": "Спасибо, данные получены. Осталось уточнить:\n\
                 1. Когда груз будет готов к отгрузке?\n\
                 2. Какой способ доставки предпочитаете?\n\
                 3. Нужна ли страховка?",
        "disposition": "draft",
        "questions": [
            "Когда груз будет готов к отгрузке?",
            "Какой способ доставки предпочитаете?",
            "Нужна ли страховка?"
        ],
        "handoff_requested": false,
        "handoff_reason": null,
        "confidence": 0.9,
        "rationale": "клиент ответил не на всё"
    })
}

fn stub_turn_one(llm: &MockLlmProvider) {
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &replies::classification_lead(),
    );
    llm.stub_json("lead qualification agent", &turn_one_qualification());
    llm.stub_json("AI assistant for MCA Logistics", &turn_one_reply());
}

fn stub_turn_two(llm: &MockLlmProvider) {
    llm.stub_json("spam detection expert", &replies::not_spam());
    llm.stub_json(
        "classification agent for a logistics",
        &replies::classification_lead(),
    );
    llm.stub_json("lead qualification agent", &turn_two_qualification());
    llm.stub_json("AI assistant for MCA Logistics", &turn_two_reply());
}

async fn drafts_for(pool: &sqlx::PgPool, lead_id: mca_mail::domain::LeadId) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM email_drafts WHERE lead_id = $1")
        .bind(lead_id)
        .fetch_one(pool)
        .await
        .expect("count drafts")
}

async fn field_value(
    pool: &sqlx::PgPool,
    lead_id: mca_mail::domain::LeadId,
    field: RequirementField,
) -> Option<String> {
    requirement_repo::get_field(pool, lead_id, field)
        .await
        .expect("read requirement")
        .and_then(|requirement| requirement.value)
}

/// The one call in the log that came from the reply agent: it is the only
/// agent whose prompt carries the "Missing information" section.
fn reply_prompt(llm: &MockLlmProvider) -> String {
    llm.call_log()
        .into_iter()
        .find(|call| call.contains("Missing information:\n"))
        .expect("the reply agent must have been called")
}

/// Slice a `label:\n<value>\n\n<next label>` section out of a prompt.
fn section<'a>(prompt: &'a str, from: &str, to: &str) -> &'a str {
    let start = prompt
        .find(from)
        .unwrap_or_else(|| panic!("prompt must contain {from:?}"))
        + from.len();
    let rest = &prompt[start..];
    let end = rest
        .find(to)
        .unwrap_or_else(|| panic!("prompt must contain {to:?}"));
    &rest[..end]
}

/// The field names the reply agent was asked to collect, in the order the
/// prompt lists them.
fn missing_information(prompt: &str) -> Vec<String> {
    section(
        prompt,
        "Missing information:\n",
        "\n\nAlready asked this customer:",
    )
    .split(", ")
    .filter(|name| !name.is_empty())
    .map(str::to_string)
    .collect()
}

fn known_facts(prompt: &str) -> String {
    section(
        prompt,
        "Already known about this order:\n",
        "\n\nMissing information:",
    )
    .to_string()
}

#[tokio::test]
async fn the_first_reply_asks_for_every_missing_field_in_one_message() {
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn_one(&llm);
    let orch = build_orchestrator(pool.clone(), llm.clone() as Arc<dyn LlmProvider>);

    let nonce = uuid::Uuid::new_v4();
    let from = format!("buyer-{nonce}@example.com");
    let subject = format!("Доставка оборудования {nonce}");

    let id = ingest(
        &pool,
        &email(
            &from,
            &subject,
            "Хотим доставить оборудование из Китая в Москву",
        ),
    )
    .await;
    orch.process_email(id).await.expect("process");

    let stored = email_repo::get(&pool, id).await.expect("get");
    assert_eq!(stored.status, EmailStatus::Processed);
    let lead_id = stored.lead_id.expect("the email must open a lead");

    // One message, one draft: the questions are not spread over turns.
    assert_eq!(
        drafts_for(&pool, lead_id).await,
        1,
        "the whole ask must leave as a single draft"
    );

    let prompt = reply_prompt(&llm);
    assert!(
        prompt.contains("ask for ALL of it in this one reply"),
        "the reply agent must be told to collect everything at once"
    );
    assert!(
        !prompt.contains("one question per reply"),
        "the one-question-per-reply rule must be gone"
    );

    let missing = missing_information(&prompt);
    for field in [
        "goods_description",
        "goods_quantity",
        "goods_weight",
        "goods_volume",
        "origin_city",
        "destination_country",
        "transport_mode",
        "desired_deadline",
        "goods_value",
        "needs_insurance",
        "needs_procurement",
        "needs_customs_clearance",
        "company_name",
        "company_inn",
        "contact_name",
        "contact_phone",
        "additional_requirements",
    ] {
        assert!(
            missing.iter().any(|name| name == field),
            "the reply prompt must still ask for {field}, got:\n{missing:?}"
        );
    }

    // What the customer already said is a fact, not a question.
    let known = known_facts(&prompt);
    assert!(
        known.contains("goods_name = оборудование"),
        "the goods the customer named must be shown as known, got:\n{known}"
    );
    for given in ["goods_name", "origin_country", "destination_city"] {
        assert!(
            !missing.iter().any(|name| name == given),
            "{given} was already given and must not be asked for again:\n{missing:?}"
        );
    }

    // The sender's address is identity, taken from the envelope, never asked.
    let contact_email: String = sqlx::query_scalar("SELECT contact_email FROM leads WHERE id = $1")
        .bind(lead_id)
        .fetch_one(&pool)
        .await
        .expect("read contact_email");
    assert_eq!(contact_email, from);
}

#[tokio::test]
async fn the_clients_answer_is_saved_and_leaves_the_missing_list() {
    let pool = setup_db().await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn_one(&llm);
    let orch = build_orchestrator(pool.clone(), llm.clone() as Arc<dyn LlmProvider>);

    let nonce = uuid::Uuid::new_v4();
    let from = format!("buyer-{nonce}@example.com");
    let subject = format!("Доставка оборудования {nonce}");

    let first = ingest(
        &pool,
        &email(
            &from,
            &subject,
            "Хотим доставить оборудование из Китая в Москву",
        ),
    )
    .await;
    orch.process_email(first).await.expect("turn one");
    let lead_id = email_repo::get(&pool, first)
        .await
        .expect("get")
        .lead_id
        .expect("the first email must open a lead");

    // One pending reply per lead is the business rule (`uq_drafts_live_per_lead`):
    // let turn one's draft leave the building, otherwise the second message
    // could not be answered at all and the test would prove nothing.
    let first_draft = draft_repo::live_draft_for_lead(&pool, lead_id)
        .await
        .expect("read the live draft")
        .expect("turn one must leave a pending draft");
    draft_repo::mark_sent(&pool, first_draft, "mock-provider-id")
        .await
        .expect("the first reply has been delivered");

    // --- Turn two: the customer answers the list in one message. -----------
    llm.reset();
    stub_turn_two(&llm);

    let second = ingest(
        &pool,
        &email(
            &from,
            &format!("Re: {subject}"),
            "Вес 12000 кг, объём 30 м3. ООО «Рога и Копыта», ИНН 7701234567, \
             Иван Иванов, +7 495 123-45-67. Готовы к концу месяца.",
        ),
    )
    .await;
    orch.process_email(second).await.expect("turn two");

    let stored = email_repo::get(&pool, second).await.expect("get");
    assert_eq!(
        stored.lead_id,
        Some(lead_id),
        "a reply in the same thread must not open a second lead"
    );

    // The answer reached the requirements repository, field by field.
    assert_eq!(
        field_value(&pool, lead_id, RequirementField::GoodsWeight).await,
        Some("12000".to_string())
    );
    assert_eq!(
        field_value(&pool, lead_id, RequirementField::GoodsVolume).await,
        Some("30".to_string())
    );
    assert_eq!(
        field_value(&pool, lead_id, RequirementField::CompanyInn).await,
        Some("7701234567".to_string())
    );
    assert_eq!(
        field_value(&pool, lead_id, RequirementField::ContactPhone).await,
        Some("+7 495 123-45-67".to_string())
    );

    // The second prompt still carries the whole open set — minus what the
    // customer has now said, so nothing is asked for twice.
    let prompt = reply_prompt(&llm);
    let missing = missing_information(&prompt);
    for answered in [
        "goods_weight",
        "goods_volume",
        "company_name",
        "company_inn",
        "contact_name",
        "contact_phone",
    ] {
        assert!(
            !missing.iter().any(|name| name == answered),
            "{answered} was just supplied and must not be asked for again:\n{missing:?}"
        );
    }
    for still_open in [
        "transport_mode",
        "desired_deadline",
        "additional_requirements",
    ] {
        assert!(
            missing.iter().any(|name| name == still_open),
            "{still_open} is still unknown and must still be requested:\n{missing:?}"
        );
    }

    let known = known_facts(&prompt);
    assert!(
        known.contains("goods_weight = 12000"),
        "the answer must show up as a known fact, got:\n{known}"
    );

    assert_eq!(
        drafts_for(&pool, lead_id).await,
        2,
        "the second message must still get its own reply: the questions the \
         first one asked must not silence the dialogue"
    );
}
