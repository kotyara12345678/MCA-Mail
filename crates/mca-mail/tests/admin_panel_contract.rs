//! Admin panel contract: cookie sessions, pipeline switches, model profiles,
//! real email actions, search/flag filters and token usage accounting.
//!
//! These are the promises the panel UI leans on. Each one is verified
//! against a real database with the same router production serves.
//!
//! Set `MCA_TEST_DATABASE_URL` to run them; without it every test is
//! skipped. No real mailbox, no network.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;

use mca_mail::api::{self, ApiState};
use mca_mail::config::{ApiSettings, AppConfig, EmailMode, LlmProviderKind, LlmSettings};
use mca_mail::domain::{EmailAddress, EmailStatus, InboundMessage};
use mca_mail::llm::mock::MockLlmProvider;
use mca_mail::llm::LlmProvider;
use mca_mail::orchestration::{Orchestrator, OrchestratorBuilder};
use mca_mail::persistence::{api_key_repo, email_repo, run_repo, thread_repo};
use mca_mail::tools::ToolRegistry;
use mca_mail_testkit::replies;

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

fn app(pg: PgPool) -> axum::Router {
    let state = Arc::new(ApiState::new(pg.clone()));
    api::build_router(&ApiSettings::default(), state)
}

/// The router with panel credentials configured, as production wires it.
fn panel_app(pg: PgPool) -> axum::Router {
    let state = Arc::new(
        ApiState::new(pg.clone()).with_admin("admin@test.local".into(), "panel-secret".into()),
    );
    api::build_router(&ApiSettings::default(), state)
}

async fn mint(pg: &PgPool, name: &str, role: api_key_repo::Role) -> String {
    let (raw, _) = api_key_repo::create(pg, name, role, "admin-panel-tests", None)
        .await
        .expect("create key");
    raw
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    key: Option<&str>,
    cookie: Option<&str>,
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(k) = key {
        builder = builder.header("X-API-Key", k);
    }
    if let Some(c) = cookie {
        builder = builder.header("cookie", c);
    }
    let has_body = body.is_some();
    let payload = match body {
        Some(value) => Body::from(serde_json::to_vec(&value).expect("serialize")),
        None => Body::empty(),
    };
    if has_body {
        builder = builder.header("content-type", "application/json");
    }
    app.oneshot(builder.body(payload).expect("request"))
        .await
        .expect("response")
}

async fn status_of(
    app: axum::Router,
    method: &str,
    uri: &str,
    key: Option<&str>,
    cookie: Option<&str>,
    body: Option<Value>,
) -> StatusCode {
    send(app, method, uri, key, cookie, body).await.status()
}

async fn body_json(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&bytes).expect("json body")
}

/// Pull the session cookie out of `Set-Cookie` for replay.
fn session_cookie(response: &Response) -> String {
    let raw = response
        .headers()
        .get("set-cookie")
        .expect("login must set a cookie")
        .to_str()
        .expect("cookie header is ascii");
    let token = raw
        .split("mca_session=")
        .nth(1)
        .expect("cookie name")
        .split(';')
        .next()
        .expect("token");
    format!("mca_session={token}")
}

fn review_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.llm.disabled = true;
    cfg.database.url = test_database_url();
    cfg.security.email_mode = EmailMode::Review;
    cfg.security.manager_card.enabled = false;
    cfg
}

fn mock_llm_settings() -> LlmSettings {
    LlmSettings {
        provider: LlmProviderKind::Mock,
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

async fn email_status(pool: &PgPool, id: uuid::Uuid) -> EmailStatus {
    sqlx::query_scalar::<_, String>("SELECT status FROM emails WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read status")
        .parse()
        .expect("parse status")
}

fn classification(category: &str, confidence: f32) -> Value {
    json!({
        "category": category,
        "confidence": confidence,
        "explanation": "admin panel fixture",
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

/// List ids returned by the email list endpoint.
fn list_ids(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

// ---------------------------------------------------------------------------
// 1. cookie login / logout
// ---------------------------------------------------------------------------

#[tokio::test]
async fn panel_login_issues_cookie_and_logout_kills_it() {
    let Some(pg) = test_pool().await else { return };
    let viewer = mint(&pg, "panel-viewer", api_key_repo::Role::Viewer).await;

    let wrong = send(
        panel_app(pg.clone()),
        "POST",
        "/api/auth/login",
        None,
        None,
        Some(json!({"login": "admin@test.local", "password": "nope"})),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED, "wrong password");

    let right = send(
        panel_app(pg.clone()),
        "POST",
        "/api/auth/login",
        None,
        None,
        Some(json!({"login": "admin@test.local", "password": "panel-secret"})),
    )
    .await;
    assert_eq!(right.status(), StatusCode::OK, "correct credentials");
    let cookie = session_cookie(&right);
    let set_cookie = right
        .headers()
        .get("set-cookie")
        .expect("set-cookie")
        .to_str()
        .expect("ascii")
        .to_string();
    assert!(set_cookie.contains("HttpOnly"), "cookie must be HttpOnly");

    // The cookie alone must reach the API — no header needed.
    let list = send(
        panel_app(pg.clone()),
        "GET",
        "/api/v1/emails?limit=1",
        None,
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        list.status(),
        StatusCode::OK,
        "cookie authenticates API calls"
    );

    let session = body_json(
        send(
            panel_app(pg.clone()),
            "GET",
            "/api/auth/session",
            None,
            Some(&cookie),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(session["authenticated"], true);
    assert_eq!(session["role"], "admin");

    // Logout revokes the server side; the same cookie then fails.
    let out = send(
        panel_app(pg.clone()),
        "POST",
        "/api/auth/logout",
        None,
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(out.status(), StatusCode::OK);
    let after = status_of(
        panel_app(pg.clone()),
        "GET",
        "/api/v1/emails",
        None,
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(after, StatusCode::UNAUTHORIZED, "revoked session");

    // A viewer key still works — sessions and keys are independent.
    assert_eq!(
        status_of(app(pg), "GET", "/api/v1/emails", Some(&viewer), None, None).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn login_without_configured_credentials_is_503_and_session_is_public() {
    let Some(pg) = test_pool().await else { return };
    // `ApiState::new` without `with_admin` — panel disabled on purpose.
    let status = status_of(
        app(pg.clone()),
        "POST",
        "/api/auth/login",
        None,
        None,
        Some(json!({"login": "x", "password": "y"})),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    let session =
        body_json(send(app(pg), "GET", "/api/auth/session", None, None, None).await).await;
    assert_eq!(session["authenticated"], false, "session probe never 401s");
}

// ---------------------------------------------------------------------------
// 2. pipeline switches
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pipeline_switches_round_trip_through_the_settings_api() {
    let Some(pg) = test_pool().await else { return };
    let viewer = mint(&pg, "settings-viewer", api_key_repo::Role::Viewer).await;
    let operator = mint(&pg, "settings-operator", api_key_repo::Role::Operator).await;
    let app = app(pg);

    // Viewer can read but not write.
    assert_eq!(
        status_of(
            app.clone(),
            "GET",
            "/api/admin/settings",
            Some(&viewer),
            None,
            None
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(
            app.clone(),
            "PATCH",
            "/api/admin/settings",
            Some(&viewer),
            None,
            Some(json!({"poll_paused": true})),
        )
        .await,
        StatusCode::FORBIDDEN
    );

    // Operator flips a switch; the response already reflects the new state.
    let flipped: Value = body_json(
        send(
            app.clone(),
            "PATCH",
            "/api/admin/settings",
            Some(&operator),
            None,
            Some(json!({"queue_paused": true})),
        )
        .await,
    )
    .await;
    assert_eq!(flipped["queue_paused"], true, "patch echoes new state");

    // Persisted read-back through a separate GET.
    let read_back: Value = body_json(
        send(
            app.clone(),
            "GET",
            "/api/admin/settings",
            Some(&operator),
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(read_back["queue_paused"], true, "switch persisted");

    // Restore before any assertion can fail and leave the switch on.
    let restored: Value = body_json(
        send(
            app.clone(),
            "PATCH",
            "/api/admin/settings",
            Some(&operator),
            None,
            Some(json!({
                "poll_paused": false,
                "queue_paused": false,
                "outbox_paused": false,
                "automation_enabled": true,
            })),
        )
        .await,
    )
    .await;
    assert_eq!(restored["queue_paused"], false, "switch restored");
    assert_eq!(restored["automation_enabled"], true);
}

// ---------------------------------------------------------------------------
// 3. model profiles
// ---------------------------------------------------------------------------

#[tokio::test]
async fn profiles_crud_and_activation_never_leak_the_key() {
    let Some(pg) = test_pool().await else { return };
    let operator = mint(&pg, "profiles-operator", api_key_repo::Role::Operator).await;
    let app = app(pg.clone());

    // Clear any active profile left behind by an earlier run, so the
    // "single active" assertion below measures this run only.
    assert_eq!(
        status_of(
            app.clone(),
            "POST",
            "/api/admin/llm/profiles/deactivate",
            Some(&operator),
            None,
            None,
        )
        .await,
        StatusCode::OK
    );

    // `llm_profiles.name` is UNIQUE — suffix with a per-run uuid so reruns
    // never collide with rows a failed run left behind.
    let run = uuid::Uuid::new_v4().to_string();
    let profile_name = format!("contract-profile-{run}");
    let renamed_name = format!("renamed-profile-{run}");

    let full_key = "sk-super-secret-key-abcdef123456";

    // Validation: base URL must be http(s).
    let bad = status_of(
        app.clone(),
        "POST",
        "/api/admin/llm/profiles",
        Some(&operator),
        None,
        Some(json!({
            "name": "bad", "base_url": "not-a-url", "model": "m",
            "api_key": "k", "price_prompt_per_million_usd": 0.1,
            "price_completion_per_million_usd": 0.2
        })),
    )
    .await;
    assert_eq!(bad, StatusCode::BAD_REQUEST, "invalid base URL rejected");

    // Create.
    let created = send(
        app.clone(),
        "POST",
        "/api/admin/llm/profiles",
        Some(&operator),
        None,
        Some(json!({
            "name": profile_name, "base_url": "https://api.deepseek.com",
            "model": "deepseek-chat", "api_key": full_key,
            "price_prompt_per_million_usd": 0.27,
            "price_completion_per_million_usd": 1.10
        })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED, "profile created");
    let created_body = body_json(created).await;
    let id = created_body["id"].as_str().expect("id").to_string();

    // List: prefix visible, raw key never present.
    let list_text = {
        let response = send(
            app.clone(),
            "GET",
            "/api/admin/llm/profiles",
            Some(&operator),
            None,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8_lossy(&bytes).into_owned()
    };
    assert!(
        !list_text.contains(full_key),
        "the raw API key must never appear in a response"
    );
    let list: Value = serde_json::from_str(&list_text).expect("list json");
    let mine = list
        .as_array()
        .expect("array")
        .iter()
        .find(|row| row["id"] == id)
        .expect("created profile listed");
    assert!(
        mine["key_prefix"].as_str().unwrap_or("").len() >= 4,
        "a key prefix is shown instead of the key"
    );
    assert_eq!(mine["is_active"], false, "created profile is inactive");

    // Edit (empty api_key keeps the stored one).
    let patched_response = send(
        app.clone(),
        "PATCH",
        &format!("/api/admin/llm/profiles/{id}"),
        Some(&operator),
        None,
        Some(json!({"name": renamed_name, "api_key": ""})),
    )
    .await;
    let patched_status = patched_response.status();
    if patched_status != StatusCode::OK {
        let bytes = axum::body::to_bytes(patched_response.into_body(), usize::MAX)
            .await
            .unwrap_or_default();
        panic!(
            "profile patched: status {patched_status}, body: {}",
            String::from_utf8_lossy(&bytes)
        );
    }

    // Activate: exactly this profile becomes active.
    let activate = status_of(
        app.clone(),
        "POST",
        &format!("/api/admin/llm/profiles/{id}/activate"),
        Some(&operator),
        None,
        None,
    )
    .await;
    assert_eq!(activate, StatusCode::OK);
    let list: Value = body_json(
        send(
            app.clone(),
            "GET",
            "/api/admin/llm/profiles",
            Some(&operator),
            None,
            None,
        )
        .await,
    )
    .await;
    let active_ids: Vec<String> = list
        .as_array()
        .expect("array")
        .iter()
        .filter(|row| row["is_active"] == true)
        .filter_map(|row| row["id"].as_str().map(str::to_string))
        .collect();
    assert_eq!(active_ids, vec![id.clone()], "single active profile");

    // Deactivate: nobody is active.
    assert_eq!(
        status_of(
            app.clone(),
            "POST",
            "/api/admin/llm/profiles/deactivate",
            Some(&operator),
            None,
            None,
        )
        .await,
        StatusCode::OK
    );

    // Delete and confirm it is gone.
    assert_eq!(
        status_of(
            app.clone(),
            "DELETE",
            &format!("/api/admin/llm/profiles/{id}"),
            Some(&operator),
            None,
            None,
        )
        .await,
        StatusCode::OK
    );
    let after: Value = body_json(
        send(
            app.clone(),
            "GET",
            "/api/admin/llm/profiles",
            Some(&operator),
            None,
            None,
        )
        .await,
    )
    .await;
    let still_there = after
        .as_array()
        .expect("array")
        .iter()
        .any(|row| row["id"] == id);
    assert!(!still_there, "deleted profile must disappear");
}

// ---------------------------------------------------------------------------
// 4. real actions on real rows
// ---------------------------------------------------------------------------

#[tokio::test]
async fn actions_transition_a_real_email_and_ack_unknown_ids() {
    let Some(pg) = test_pool().await else { return };
    let operator = mint(&pg, "actions-operator", api_key_repo::Role::Operator).await;
    let manager = mint(&pg, "actions-manager", api_key_repo::Role::Manager).await;
    let id = ingest(
        &pg,
        &email(
            &format!("client-{}@example.com", uuid::Uuid::new_v4()),
            "Договор на перевозку",
            "Просим рассмотреть договор.",
        ),
    )
    .await;
    let app = app(pg.clone());

    // Reprocess: real row goes back to pending, message matches the contract.
    let response = send(
        app.clone(),
        "POST",
        &format!("/api/v1/emails/{id}/reprocess"),
        Some(&operator),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["message"], format!("reprocessing scheduled for {id}"));
    assert_eq!(email_status(&pg, id).await, EmailStatus::Pending);

    // Review → needs_review.
    let response = send(
        app.clone(),
        "POST",
        &format!("/api/v1/emails/{id}/review"),
        Some(&operator),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["message"], format!("email {id} flagged for review"));
    assert_eq!(email_status(&pg, id).await, EmailStatus::NeedsReview);

    // Approve needs manager: operator is refused, manager is not.
    assert_eq!(
        status_of(
            app.clone(),
            "POST",
            &format!("/api/v1/emails/{id}/approve"),
            Some(&operator),
            None,
            None,
        )
        .await,
        StatusCode::FORBIDDEN
    );
    let response = send(
        app.clone(),
        "POST",
        &format!("/api/v1/emails/{id}/approve"),
        Some(&manager),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["message"], format!("email {id} approved"));
    assert_eq!(email_status(&pg, id).await, EmailStatus::Processed);

    // Reject → quarantined.
    let response = send(
        app.clone(),
        "POST",
        &format!("/api/v1/emails/{id}/reject"),
        Some(&manager),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(email_status(&pg, id).await, EmailStatus::Quarantined);

    // An id that does not exist still gets a 200 acknowledgement — scripts
    // fire these in batches and one stale id must not read as a failure.
    let missing = uuid::Uuid::new_v4();
    let response = send(
        app,
        "POST",
        &format!("/api/v1/emails/{missing}/reprocess"),
        Some(&operator),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(
        body["message"],
        format!("reprocessing scheduled for {missing}")
    );
}

// ---------------------------------------------------------------------------
// 5. search and red-flag filters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_filters_on_flag_and_search_text() {
    let Some(pg) = test_pool().await else { return };
    let viewer = mint(&pg, "filters-viewer", api_key_repo::Role::Viewer).await;
    let marker = format!("контракт-{}", uuid::Uuid::new_v4());
    let from = format!("flag-{}@example.com", uuid::Uuid::new_v4());
    let flagged_id = ingest(
        &pg,
        &email(&from, &format!("Заявка {marker}"), "нужны две фуры"),
    )
    .await;
    email_repo::raise_order_flag(&pg, flagged_id)
        .await
        .expect("raise flag");
    let plain_id = ingest(
        &pg,
        &email(
            &format!("plain-{}@example.com", uuid::Uuid::new_v4()),
            "Поздравление с праздником",
            "всего доброго",
        ),
    )
    .await;
    let app = app(pg);

    // flagged=true keeps only the red-flagged row.
    let list: Value = body_json(
        send(
            app.clone(),
            "GET",
            "/api/v1/emails?flagged=true&limit=200",
            Some(&viewer),
            None,
            None,
        )
        .await,
    )
    .await;
    let ids = list_ids(list["items"].as_array().expect("items"));
    assert!(
        ids.contains(&flagged_id.to_string()),
        "flagged row must match flagged=true"
    );
    assert!(
        !ids.contains(&plain_id.to_string()),
        "unflagged row must not match flagged=true"
    );

    // Search hits the subject substring.
    let list: Value = body_json(
        send(
            app.clone(),
            "GET",
            &format!("/api/v1/emails?search={marker}&limit=200"),
            Some(&viewer),
            None,
            None,
        )
        .await,
    )
    .await;
    let ids = list_ids(list["items"].as_array().expect("items"));
    assert_eq!(
        ids,
        vec![flagged_id.to_string()],
        "search narrows to the hit"
    );

    // flagged=false is the complement.
    let list: Value = body_json(
        send(
            app,
            "GET",
            "/api/v1/emails?flagged=false&limit=200",
            Some(&viewer),
            None,
            None,
        )
        .await,
    )
    .await;
    let ids = list_ids(list["items"].as_array().expect("items"));
    assert!(
        !ids.contains(&flagged_id.to_string()),
        "flagged row excluded from flagged=false"
    );
}

// ---------------------------------------------------------------------------
// 6. usage accounting lands in agent_runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn llm_calls_are_recorded_for_the_cost_page() {
    let Some(pg) = test_pool().await else { return };
    let viewer = mint(&pg, "usage-viewer", api_key_repo::Role::Viewer).await;
    let llm = Arc::new(MockLlmProvider::new(&mock_llm_settings()));
    stub_turn(
        &llm,
        &automated_notification(),
        &classification("transport_request", 0.93),
    );
    let orch = build_orchestrator(pg.clone(), llm as Arc<dyn LlmProvider>);

    let id = ingest(
        &pg,
        &email(
            &format!("usage-{}@example.com", uuid::Uuid::new_v4()),
            "Перевозка Шанхай — Москва",
            "Интересует ставка за 40HQ, отгрузка в ноябре.",
        ),
    )
    .await;
    orch.process_email(id).await.expect("pipeline run");

    // At least one LLM call is now a durable row tied to this email.
    let runs = run_repo::agent_runs_for_email(&pg, id, 50)
        .await
        .expect("agent runs");
    assert!(
        !runs.is_empty(),
        "process_email must record agent runs for usage accounting"
    );
    let succeeded = runs.iter().filter(|r| r.state == "succeeded").count();
    assert!(succeeded > 0, "successful calls are recorded as succeeded");

    // The usage endpoint aggregates them for the panel.
    let usage_response = send(
        app(pg.clone()),
        "GET",
        "/api/admin/usage?days=30",
        Some(&viewer),
        None,
        None,
    )
    .await;
    let usage_status = usage_response.status();
    let usage_text = {
        let bytes = axum::body::to_bytes(usage_response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8_lossy(&bytes).into_owned()
    };
    assert_eq!(
        usage_status,
        StatusCode::OK,
        "usage endpoint status; body: {usage_text}"
    );
    let usage: Value = serde_json::from_str(&usage_text).expect("usage json");
    assert!(
        usage["total"]["calls"].as_i64().unwrap_or(0) >= 1,
        "usage totals count the recorded calls; body: {usage_text}"
    );
    assert!(usage["by_day"].is_array());
    assert!(usage["by_model"].is_array());

    // The dashboard summary answers with every section the SPA renders.
    let summary: Value = body_json(
        send(
            app(pg.clone()),
            "GET",
            "/api/admin/summary",
            Some(&viewer),
            None,
            None,
        )
        .await,
    )
    .await;
    assert!(summary["emails"]["total"].as_i64().is_some());
    assert!(summary["pipeline"]["poll_paused"].is_boolean());
    assert!(summary["llm"]["active_model"].is_string());
    assert!(summary["cost_total_micros"].as_i64().is_some());

    // Per-email trace for the detail view.
    let trace: Value = body_json(
        send(
            app(pg),
            "GET",
            &format!("/api/admin/emails/{id}/runs?limit=20"),
            Some(&viewer),
            None,
            None,
        )
        .await,
    )
    .await;
    assert!(
        trace["agent_runs"]
            .as_array()
            .map(|rows| !rows.is_empty())
            .unwrap_or(false),
        "detail view shows the LLM calls of this email"
    );
}

// ---------------------------------------------------------------------------
// 7. SPA shell
// ---------------------------------------------------------------------------

#[tokio::test]
async fn spa_shell_answers_pages_and_404s_json_for_api_typos() {
    let Some(pg) = test_pool().await else { return };
    let app = app(pg);

    let index = send(app.clone(), "GET", "/", None, None, None).await;
    assert_eq!(index.status(), StatusCode::OK);
    let content_type = index
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(content_type.contains("text/html"), "shell is html");

    let js = send(app.clone(), "GET", "/static/app.js", None, None, None).await;
    assert_eq!(js.status(), StatusCode::OK);
    let css = send(app.clone(), "GET", "/static/style.css", None, None, None).await;
    assert_eq!(css.status(), StatusCode::OK);

    // A deep link refresh falls back to the shell (GET, non-API path).
    let deep = send(app.clone(), "GET", "/any/spa/route", None, None, None).await;
    assert_eq!(deep.status(), StatusCode::OK, "SPA fallback serves pages");

    // A typo'd API path stays JSON 404 — HTML there would confuse scripts.
    let typo = send(app, "GET", "/api/no/such/endpoint", None, None, None).await;
    assert_eq!(typo.status(), StatusCode::NOT_FOUND);
}
