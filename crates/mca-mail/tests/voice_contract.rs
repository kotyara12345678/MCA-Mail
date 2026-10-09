//! Voice contract: the order card queued when a call finishes (§5) and the
//! dialogue state the call agent sees on a resumed call (§6).
//!
//! §5 — four claims have to hold before a customer can be promised an email:
//! a dictated address is validated and stored normalized, a call without a
//! usable address leaves a recorded *reason* instead of a silent nothing,
//! quote-blocking gaps do the same, and a complete lead produces exactly one
//! outbox row — replayed finishes collapse onto it. Queuing is not sending:
//! the row stays `queued` for the send worker's policy check.
//!
//! §6 — the same subscriber always resumes the same lead (`8…` and `+7…`
//! normalize alike), and `GET …/requirements` reports the questions asked so
//! far with their answered state, so a repeat call continues instead of
//! opening with the same first question.
//!
//! Set `MCA_TEST_DATABASE_URL` to run them; without it the module is skipped.

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
use mca_mail::config::{ApiSettings, DatabaseSettings};
use mca_mail::persistence::api_key_repo;

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

async fn app(pg: PgPool) -> axum::Router {
    let state = Arc::new(ApiState::new(pg.clone()));
    api::build_router(&ApiSettings::default(), state)
}

async fn mint(pg: &PgPool, name: &str) -> String {
    let (raw, _) = api_key_repo::create(
        pg,
        name,
        api_key_repo::Role::Operator,
        "voice_contract",
        None,
    )
    .await
    .expect("create key");
    raw
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    key: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("X-API-Key", key);
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

static PHONE_SEQ: AtomicU32 = AtomicU32::new(0);

/// A caller number unique per call, shaped like a real Russian mobile
/// number (11 digits starting with `7`): time keeps runs apart, the counter
/// keeps parallel tests in one run apart. 11 digits matters — that is the
/// shape the `8…` → `7…` continuation normalization rewrites.
fn fresh_phone() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis()
        % 10_000_000;
    let seq = PHONE_SEQ.fetch_add(1, Ordering::Relaxed) % 100;
    format!("79{stamp:07}{seq:02}")
}

async fn create_lead(app: &axum::Router, key: &str, phone: &str) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/api/v1/voice/leads",
        key,
        Some(json!({ "caller_number": phone })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create lead: {body}");
    body["lead_id"].as_str().expect("lead_id").to_string()
}

async fn finish(
    app: &axum::Router,
    key: &str,
    lead_id: &str,
    call_id: &str,
) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/api/v1/voice/calls/finish",
        key,
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
    .await
}

/// The latest audit row for this lead's card attempt, `(outcome, reason)`.
async fn audit(pool: &PgPool, lead_id: &str) -> Option<(String, Option<String>)> {
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT outcome, details->>'reason' FROM audit_logs \
         WHERE action = 'voice_order_card' AND resource_id = $1 \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(lead_id)
    .fetch_optional(pool)
    .await
    .expect("audit query");
    row
}

#[tokio::test]
async fn order_card_contract() {
    let Some(pg) = test_pool().await else { return };
    let app = app(pg.clone()).await;
    let key = mint(&pg, "voice-order-card").await;

    // --- 1. an unusable address is refused at the door ---------------------
    let lead_a = create_lead(&app, &key, &fresh_phone()).await;
    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/v1/voice/leads/{lead_a}"),
        &key,
        Some(json!({ "contact_email": "not-an-email" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "bad email must be 400: {body}"
    );

    // --- 2. no email yet → skipped, reason recorded ------------------------
    let (status, body) = finish(&app, &key, &lead_a, "call-a1").await;
    assert_eq!(status, StatusCode::OK, "finish: {body}");
    assert_eq!(body["order_card"]["state"], "skipped");
    assert_eq!(body["order_card"]["reason"], "missing_email");
    let (outcome, reason) = audit(&pg, &lead_a).await.expect("audit row");
    assert_eq!(outcome, "denied");
    assert_eq!(reason.as_deref(), Some("missing_email"));

    // --- 3. a dictated address is stored normalized ------------------------
    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/v1/voice/leads/{lead_a}"),
        &key,
        Some(json!({ "contact_email": "  Buyer@Acme.TEST " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch email: {body}");

    // --- 4. blocking gaps are a reason, not a half-addressed card ----------
    let (status, body) = finish(&app, &key, &lead_a, "call-a2").await;
    assert_eq!(status, StatusCode::OK, "finish: {body}");
    assert_eq!(body["order_card"]["state"], "skipped");
    assert_eq!(body["order_card"]["reason"], "incomplete_requirements");
    let missing = body["order_card"]["missing_fields"]
        .as_array()
        .expect("missing_fields");
    assert!(!missing.is_empty(), "blocking gaps must be listed");
    let (outcome, reason) = audit(&pg, &lead_a).await.expect("audit row");
    assert_eq!(outcome, "denied");
    assert_eq!(reason.as_deref(), Some("incomplete_requirements"));

    // --- 5. fill every blocking gap → the card is queued -------------------
    let (_, requirements) = call(
        &app,
        "GET",
        &format!("/api/v1/voice/leads/{lead_a}/requirements"),
        &key,
        None,
    )
    .await;
    let gaps: Vec<Value> = requirements["blocking_gaps"]
        .as_array()
        .expect("blocking_gaps")
        .clone();
    let facts: Vec<Value> = gaps
        .iter()
        .map(|gap| json!({ "field": gap["field"], "value": "12" }))
        .collect();
    let (status, body) = call(
        &app,
        "PUT",
        &format!("/api/v1/voice/leads/{lead_a}/requirements"),
        &key,
        Some(json!({ "requirements": facts })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "save requirements: {body}");
    assert_eq!(body["blocking_gaps"], json!([]), "gaps must be closed");

    let (status, body) = finish(&app, &key, &lead_a, "call-a3").await;
    assert_eq!(status, StatusCode::OK, "finish: {body}");
    assert_eq!(body["order_card"]["state"], "queued");
    let outbox_id = body["order_card"]["outbox_id"]
        .as_str()
        .expect("outbox_id")
        .to_string();
    let outbox_uuid = outbox_id.parse::<uuid::Uuid>().expect("uuid");

    // The row is a normal outbox intent: normalized recipient, still queued —
    // delivery is the send worker's policy decision, not this endpoint's.
    let row: (String, String, String, String) = sqlx::query_as(
        "SELECT message_type, recipient, status, idempotency_key \
         FROM mailbox_outbox WHERE id = $1",
    )
    .bind(outbox_uuid)
    .fetch_one(&pg)
    .await
    .expect("outbox row");
    assert_eq!(row.0, "order_card");
    assert_eq!(row.1, "buyer@acme.test");
    assert_eq!(row.2, "queued");
    assert!(row.3.starts_with(&format!("order_card:{lead_a}:")));

    let (outcome, reason) = audit(&pg, &lead_a).await.expect("audit row");
    assert_eq!(outcome, "success");
    assert_eq!(reason, None, "a queued card carries no skip reason");

    // --- 6. a replayed finish collapses onto the same row ------------------
    let (status, body) = finish(&app, &key, &lead_a, "call-a4").await;
    assert_eq!(status, StatusCode::OK, "finish: {body}");
    assert_eq!(body["order_card"]["state"], "already_queued");
    let lead_uuid = lead_a.parse::<uuid::Uuid>().expect("uuid");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailbox_outbox \
         WHERE lead_id = $1 AND message_type = 'order_card'",
    )
    .bind(lead_uuid)
    .fetch_one(&pg)
    .await
    .expect("count");
    assert_eq!(count, 1, "one card per lead per recipient, ever");
}

#[tokio::test]
async fn dialogue_state_and_lead_continuation_contract() {
    let Some(pg) = test_pool().await else { return };
    let app = app(pg.clone()).await;
    let key = mint(&pg, "voice-asked-questions").await;

    // --- 1. one subscriber, two dial formats → one lead --------------------
    let phone = fresh_phone();
    let lead = create_lead(&app, &key, &phone).await;
    let trunk_eight = format!("8{}", &phone[1..]);
    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/voice/leads",
        &key,
        Some(json!({ "caller_number": trunk_eight })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "recreate: {body}");
    assert_eq!(
        body["created"], false,
        "the same subscriber must resume the lead: {body}"
    );
    assert_eq!(body["lead_id"].as_str().expect("lead_id"), lead);

    // --- 2. a fresh lead has been asked nothing yet ------------------------
    let (status, requirements) = call(
        &app,
        "GET",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        &key,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "requirements: {requirements}");
    assert_eq!(requirements["asked_questions"], json!([]));

    // --- 3. a dialogue the agent walked through ----------------------------
    let turns = [
        ("outbound", "Здравствуйте, записываю."),
        ("outbound", "Сколько груза и что за товар?"),
        ("outbound", "сколько груза и что за товар?"),
        ("inbound", "Двенадцать тонн запчастей."),
        ("outbound", "Откуда забирать?"),
    ];
    for (direction, body) in turns {
        let (status, response) = call(
            &app,
            "POST",
            &format!("/api/v1/voice/leads/{lead}/conversation"),
            &key,
            Some(json!({
                "direction": direction,
                "body": body,
                "idempotency_key": uuid::Uuid::new_v4().to_string(),
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "append `{body}`: {response}");
    }

    // --- 4. the requirements view reports the asked questions --------------
    //
    // The greeting is not a question; the case-variant repeat collapses onto
    // one entry; the question answered by the customer carries answered=true,
    // the one still open carries false. Chronological order.
    let (status, requirements) = call(
        &app,
        "GET",
        &format!("/api/v1/voice/leads/{lead}/requirements"),
        &key,
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
    assert_eq!(asked[0]["question"], "Сколько груза и что за товар?");
    assert_eq!(asked[0]["answered"], true);
    assert_eq!(asked[1]["question"], "Откуда забирать?");
    assert_eq!(asked[1]["answered"], false);
}
