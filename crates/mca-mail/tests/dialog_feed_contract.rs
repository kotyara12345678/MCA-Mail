//! Dialog-feed contract: the thread list, the chronological block feed and
//! the runtime agent-mode switch the panel flips.
//!
//! These are the promises the redesigned panel leans on. Each one is verified
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
use mca_mail::config::ApiSettings;
use mca_mail::domain::{DraftStatus, EmailAddress, EmailDraft, InboundMessage};
use mca_mail::persistence::{api_key_repo, draft_repo, email_repo, thread_repo};

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

fn app(pg: PgPool) -> axum::Router {
    let state = Arc::new(ApiState::new(pg));
    api::build_router(&ApiSettings::default(), state)
}

async fn mint(pg: &PgPool, name: &str, role: api_key_repo::Role) -> String {
    let (raw, _) = api_key_repo::create(pg, name, role, "dialog-feed-tests", None)
        .await
        .expect("create key");
    raw
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(k) = key {
        builder = builder.header("X-API-Key", k);
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

async fn body_json(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&bytes).expect("json body")
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

/// The kinds of a feed page, in the order the API returned them.
fn kinds(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|row| row["kind"].as_str().map(str::to_string))
        .collect()
}

/// `at` of the first (oldest) block on a page — the cursor for "load older".
fn oldest_at(items: &[Value]) -> String {
    items
        .first()
        .and_then(|row| row["at"].as_str())
        .expect("first block with a timestamp")
        .to_string()
}

// ---------------------------------------------------------------------------
// 1. list + feed + pagination
// ---------------------------------------------------------------------------

#[tokio::test]
async fn thread_list_and_feed_serve_one_chronological_conversation() {
    let Some(pg) = test_pool().await else { return };
    let viewer = mint(&pg, "feed-viewer", api_key_repo::Role::Viewer).await;
    let marker = format!("лента-{}", uuid::Uuid::new_v4());
    let from = format!("feed-{}@example.com", uuid::Uuid::new_v4());
    let subject = format!("Заявка {marker}");

    let first = ingest(&pg, &email(&from, &subject, "Нужны две фуры до Твери.")).await;
    let second = ingest(
        &pg,
        &email(&from, &subject, "Уточнение: выезд в понедельник."),
    )
    .await;

    // One thread: the two messages chain by address + normalized subject.
    let thread_id: uuid::Uuid = sqlx::query_scalar("SELECT thread_id FROM emails WHERE id = $1")
        .bind(first)
        .fetch_one(&pg)
        .await
        .expect("thread of first");
    let thread_of_second: uuid::Uuid =
        sqlx::query_scalar("SELECT thread_id FROM emails WHERE id = $1")
            .bind(second)
            .fetch_one(&pg)
            .await
            .expect("thread of second");
    assert_eq!(thread_id, thread_of_second, "same conversation");

    // A draft the agent prepared, answered after both messages. The repo
    // assigns the real id, so the assertion code works with what it returns.
    let (draft_id, _) = draft_repo::create(
        &pg,
        &EmailDraft {
            id: uuid::Uuid::new_v4(),
            lead_id: None,
            email_id: Some(second),
            in_reply_to: None,
            to_addresses: vec![from.clone()],
            cc_addresses: vec![],
            subject: subject.clone(),
            body: "Ответ агента: выезд возможен в понедельник.".to_string(),
            status: DraftStatus::PendingApproval,
            idempotency_key: format!("feed-test-{}", uuid::Uuid::new_v4()),
            suppression_reason: None,
            reviewed_by: None,
            reviewed_at: None,
            sent_at: None,
            provider_message_id: None,
            created_at: chrono::Utc::now(),
        },
    )
    .await
    .expect("draft");

    // A warning event tied to the second message.
    sqlx::query(
        "INSERT INTO processing_events (email_id, severity, code, message) \
         VALUES ($1, 'warning', 'feed.test', 'diagnostic for the feed')",
    )
    .bind(second)
    .execute(&pg)
    .await
    .expect("event");

    // Fixed, strictly increasing timestamps so the assertions below are
    // deterministic rather than racing the clock.
    let base = chrono::Utc::now() - chrono::Duration::hours(4);
    sqlx::query("UPDATE emails SET received_at = $2, status = 'processed' WHERE id = $1")
        .bind(first)
        .bind(base)
        .execute(&pg)
        .await
        .expect("time first");
    sqlx::query("UPDATE emails SET received_at = $2, status = 'processed' WHERE id = $1")
        .bind(second)
        .bind(base + chrono::Duration::hours(1))
        .execute(&pg)
        .await
        .expect("time second");
    sqlx::query("UPDATE email_drafts SET created_at = $2 WHERE id = $1")
        .bind(draft_id)
        .bind(base + chrono::Duration::hours(2))
        .execute(&pg)
        .await
        .expect("time draft");
    sqlx::query(
        "UPDATE processing_events SET created_at = $2 WHERE email_id = $1 AND code = 'feed.test'",
    )
    .bind(second)
    .bind(base + chrono::Duration::hours(3))
    .execute(&pg)
    .await
    .expect("time event");

    let app = app(pg.clone());

    // --- list --------------------------------------------------------------
    let list: Value = body_json(
        send(
            app.clone(),
            "GET",
            &format!("/api/admin/threads?search={marker}"),
            Some(&viewer),
            None,
        )
        .await,
    )
    .await;
    let items = list["items"]
        .as_array()
        .unwrap_or_else(|| panic!("thread list items: {list}"));
    assert_eq!(items.len(), 1, "one thread for the marker");
    let row = &items[0];
    assert_eq!(row["id"], json!(thread_id));
    assert_eq!(row["message_count"], 2, "both inbound messages counted");
    assert_eq!(row["inbound_count"], 2);
    assert_eq!(row["draft_count"], 1, "the agent's reply counted");
    assert_eq!(row["pending_drafts"], 1, "waiting for a human decision");
    assert_eq!(row["needs_attention"], true, "a pending draft is attention");
    assert_eq!(row["flagged"], false);
    assert_eq!(
        row["participant_address"].as_str(),
        Some(from.as_str()),
        "the counterpart is who writes to us"
    );
    assert!(row["subject"].as_str().unwrap().contains(&marker));
    assert!(list["total"].as_i64().unwrap_or(0) >= 1);

    // attention filter keeps it (pending draft), all keeps everything.
    let attention: Value = body_json(
        send(
            app.clone(),
            "GET",
            "/api/admin/threads?status=attention&limit=200",
            Some(&viewer),
            None,
        )
        .await,
    )
    .await;
    let ids: Vec<String> = attention["items"]
        .as_array()
        .expect("attention items")
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_string))
        .collect();
    assert!(
        ids.contains(&thread_id.to_string()),
        "pending draft must keep the thread in the attention queue"
    );

    // --- feed: full chronological page -------------------------------------
    let feed: Value = body_json(
        send(
            app.clone(),
            "GET",
            &format!("/api/admin/threads/{thread_id}"),
            Some(&viewer),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(feed["thread"]["id"], json!(thread_id));
    assert_eq!(feed["lead"], Value::Null, "no lead was attached");
    let page = feed["items"].as_array().expect("feed items");
    assert_eq!(
        kinds(page),
        vec!["message", "message", "draft", "event"],
        "one conversation, blocks in chronological order"
    );
    assert_eq!(feed["has_more"], false, "the whole thread fits one page");

    // Block-level contract: the inbound mail keeps sender and body, the
    // draft carries the status the reviewer acts on.
    assert_eq!(page[0]["direction"], "inbound");
    assert_eq!(page[0]["from_address"], json!(from));
    assert!(page[0]["body"].as_str().unwrap().contains("две фуры"));
    assert_eq!(page[2]["status"], "pending_approval");
    assert_eq!(page[2]["generated_by"], "ai");
    assert_eq!(page[2]["to"], json!([from]));
    assert_eq!(page[3]["severity"], "warning");
    assert_eq!(page[3]["code"], "feed.test");

    // Timestamps never run backwards in a feed.
    let times: Vec<&str> = page.iter().filter_map(|row| row["at"].as_str()).collect();
    let mut sorted = times.clone();
    sorted.sort();
    assert_eq!(times, sorted, "chronological order");

    // --- feed: paging walks strictly older ---------------------------------
    let paged: Value = body_json(
        send(
            app.clone(),
            "GET",
            &format!("/api/admin/threads/{thread_id}?limit=1"),
            Some(&viewer),
            None,
        )
        .await,
    )
    .await;
    let page = paged["items"].as_array().expect("page items");
    assert_eq!(kinds(page), vec!["event"], "newest block first");
    assert_eq!(paged["has_more"], true);

    // A RFC 3339 cursor carries `+`, which a query string decodes as a space:
    // the client must percent-encode it, as any HTTP client would.
    let mut cursor = oldest_at(page).replace('+', "%2B");
    let mut seen: Vec<String> = kinds(page);
    for _ in 0..3 {
        let older: Value = body_json(
            send(
                app.clone(),
                "GET",
                &format!("/api/admin/threads/{thread_id}?limit=1&before={cursor}"),
                Some(&viewer),
                None,
            )
            .await,
        )
        .await;
        let page = older["items"]
            .as_array()
            .unwrap_or_else(|| panic!("older items: {older}"));
        if page.is_empty() {
            assert_eq!(older["has_more"], false, "empty page ends the history");
            break;
        }
        seen.extend(kinds(page));
        cursor = oldest_at(page).replace('+', "%2B");
    }
    assert_eq!(
        seen,
        vec!["event", "draft", "message", "message"],
        "older pages walk back through the whole conversation"
    );

    // --- events filter ------------------------------------------------------
    let no_events: Value = body_json(
        send(
            app.clone(),
            "GET",
            &format!("/api/admin/threads/{thread_id}?events=off"),
            Some(&viewer),
            None,
        )
        .await,
    )
    .await;
    let page = no_events["items"].as_array().expect("items");
    assert_eq!(
        kinds(page),
        vec!["message", "message", "draft"],
        "events=off hides service blocks"
    );

    // --- error handling -----------------------------------------------------
    let missing = send(
        app.clone(),
        "GET",
        &format!("/api/admin/threads/{}", uuid::Uuid::new_v4()),
        Some(&viewer),
        None,
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND, "unknown thread");

    let bad_before = send(
        app.clone(),
        "GET",
        &format!("/api/admin/threads/{thread_id}?before=yesterday"),
        Some(&viewer),
        None,
    )
    .await;
    assert_eq!(
        bad_before.status(),
        StatusCode::BAD_REQUEST,
        "unparsable cursor rejected"
    );

    let anonymous = send(app, "GET", "/api/admin/threads", None, None).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// 1b. the orders filter — the panel's default view
// ---------------------------------------------------------------------------

#[tokio::test]
async fn orders_filter_shows_only_red_flagged_threads() {
    let Some(pg) = test_pool().await else { return };
    let app = app(pg.clone());
    let viewer = mint(&pg, "orders-filter-viewer", api_key_repo::Role::Viewer).await;

    // One thread with the red flag (the panel's "Заказы" view), one without.
    let marked = ingest(
        &pg,
        &email(
            &format!("orders-{}@client.ru", uuid::Uuid::new_v4()),
            "Заказ на перевозку",
            "Нужны три фуры до Казани.",
        ),
    )
    .await;
    email_repo::raise_order_flag(&pg, marked)
        .await
        .expect("raise flag");
    let plain = ingest(
        &pg,
        &email(
            &format!("noise-{}@client.ru", uuid::Uuid::new_v4()),
            "Подписка на рассылку",
            "Предлагаем услуги грузоперевозок по всей России.",
        ),
    )
    .await;

    let flagged_thread: uuid::Uuid =
        sqlx::query_scalar("SELECT thread_id FROM emails WHERE id = $1")
            .bind(marked)
            .fetch_one(&pg)
            .await
            .expect("thread of flagged");
    let plain_thread: uuid::Uuid = sqlx::query_scalar("SELECT thread_id FROM emails WHERE id = $1")
        .bind(plain)
        .fetch_one(&pg)
        .await
        .expect("thread of plain");

    let response = send(
        app,
        "GET",
        "/api/admin/threads?flagged=true&limit=200",
        Some(&viewer),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let ids: Vec<String> = body["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_string))
        .collect();
    assert!(
        ids.contains(&flagged_thread.to_string()),
        "flagged thread visible under flagged=true: {ids:?}",
    );
    assert!(
        !ids.contains(&plain_thread.to_string()),
        "unflagged thread hidden under flagged=true: {ids:?}",
    );
}

// ---------------------------------------------------------------------------
// 2. attention lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attention_queue_follows_the_draft_and_email_states() {
    let Some(pg) = test_pool().await else { return };
    let viewer = mint(&pg, "attention-viewer", api_key_repo::Role::Viewer).await;
    // Approving a reply is a manager's decision, not an operator's. The
    // reviewer is recorded as the key's prefix, which is what the API echoes.
    let manager = mint(&pg, "attention-manager", api_key_repo::Role::Manager).await;
    let manager_prefix = api_key_repo::key_prefix(&manager);
    let marker = format!("внимание-{}", uuid::Uuid::new_v4());
    let from = format!("attention-{}@example.com", uuid::Uuid::new_v4());

    let email_id = ingest(
        &pg,
        &email(
            &from,
            &format!("Заявка {marker}"),
            "Срочно нужен транспорт.",
        ),
    )
    .await;
    sqlx::query("UPDATE emails SET status = 'processed' WHERE id = $1")
        .bind(email_id)
        .execute(&pg)
        .await
        .expect("processed");

    let (draft_id, _) = draft_repo::create(
        &pg,
        &EmailDraft {
            id: uuid::Uuid::new_v4(),
            lead_id: None,
            email_id: Some(email_id),
            in_reply_to: None,
            to_addresses: vec![from.clone()],
            cc_addresses: vec![],
            subject: format!("Заявка {marker}"),
            body: "Готовы предложить ставку.".to_string(),
            status: DraftStatus::PendingApproval,
            idempotency_key: format!("attention-{}", uuid::Uuid::new_v4()),
            suppression_reason: None,
            reviewed_by: None,
            reviewed_at: None,
            sent_at: None,
            provider_message_id: None,
            created_at: chrono::Utc::now(),
        },
    )
    .await
    .expect("draft");

    let app = app(pg.clone());
    let url = format!("/api/admin/threads?search={marker}&status=attention");

    let before: Value = body_json(send(app.clone(), "GET", &url, Some(&viewer), None).await).await;
    assert_eq!(
        before["items"]
            .as_array()
            .map(|rows| rows.len())
            .unwrap_or(0),
        1,
        "processed mail + pending draft still needs attention; body: {before}"
    );

    // Operator approves the draft: the queue empties.
    let approved = send(
        app.clone(),
        "POST",
        &format!("/api/admin/drafts/{draft_id}/approve"),
        Some(&manager),
        None,
    )
    .await;
    assert_eq!(approved.status(), StatusCode::OK, "draft approved");

    let after: Value = body_json(send(app.clone(), "GET", &url, Some(&viewer), None).await).await;
    assert_eq!(
        after["items"]
            .as_array()
            .map(|rows| rows.len())
            .unwrap_or(0),
        0,
        "an approved draft is no longer waiting"
    );

    // The feed reflects the decision without a reload of any other view.
    let thread_id: uuid::Uuid = sqlx::query_scalar("SELECT thread_id FROM emails WHERE id = $1")
        .bind(email_id)
        .fetch_one(&pg)
        .await
        .expect("thread");
    let feed: Value = body_json(
        send(
            app,
            "GET",
            &format!("/api/admin/threads/{thread_id}"),
            Some(&viewer),
            None,
        )
        .await,
    )
    .await;
    let draft_block = feed["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|row| row["kind"] == "draft")
        .expect("draft block present");
    assert_eq!(draft_block["status"], "approved");
    assert_eq!(
        draft_block["reviewed_by"].as_str(),
        Some(manager_prefix.as_str()),
        "reviewer recorded"
    );
    assert!(
        draft_block["reviewed_at"].is_string(),
        "decision time recorded"
    );
}

// ---------------------------------------------------------------------------
// 3. runtime agent mode
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_mode_round_trips_and_rejects_unknown_values() {
    let Some(pg) = test_pool().await else { return };
    let operator = mint(&pg, "mode-operator", api_key_repo::Role::Operator).await;
    let viewer = mint(&pg, "mode-viewer", api_key_repo::Role::Viewer).await;
    let app = app(pg.clone());

    // Start from a known state: no stored override.
    sqlx::query("DELETE FROM system_settings WHERE key = 'agent.mode'")
        .execute(&pg)
        .await
        .expect("clear override");

    let initial: Value = body_json(
        send(
            app.clone(),
            "GET",
            "/api/admin/settings",
            Some(&viewer),
            None,
        )
        .await,
    )
    .await;
    let initial_mode = initial["email_mode"].as_str().expect("email_mode");
    assert_eq!(
        initial_mode, "dry_run",
        "boot default without a stored override is dry_run"
    );

    // Viewer may read but never change the mode.
    assert_eq!(
        send(
            app.clone(),
            "PATCH",
            "/api/admin/settings",
            Some(&viewer),
            Some(json!({"email_mode": "review"})),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    // Operator switches the agent into review.
    let flipped: Value = body_json(
        send(
            app.clone(),
            "PATCH",
            "/api/admin/settings",
            Some(&operator),
            Some(json!({"email_mode": "review"})),
        )
        .await,
    )
    .await;
    let flipped_mode = flipped["email_mode"].as_str().expect("email_mode");
    assert_eq!(flipped_mode, "review", "patch echoes the new mode");

    // Persisted read-back.
    let read_back: Value = body_json(
        send(
            app.clone(),
            "GET",
            "/api/admin/settings",
            Some(&operator),
            None,
        )
        .await,
    )
    .await;
    let read_back_mode = read_back["email_mode"].as_str().expect("email_mode");
    assert_eq!(read_back_mode, "review", "mode persisted");

    // The dashboard summary carries the same answer.
    let summary: Value = body_json(
        send(
            app.clone(),
            "GET",
            "/api/admin/summary",
            Some(&viewer),
            None,
        )
        .await,
    )
    .await;
    let summary_mode = summary["pipeline"]["email_mode"]
        .as_str()
        .expect("summary email_mode");
    assert_eq!(summary_mode, "review", "summary agrees with settings");

    // Auto mode is reachable too.
    let auto: Value = body_json(
        send(
            app.clone(),
            "PATCH",
            "/api/admin/settings",
            Some(&operator),
            Some(json!({"email_mode": "auto"})),
        )
        .await,
    )
    .await;
    let auto_mode = auto["email_mode"].as_str().expect("email_mode");
    assert_eq!(auto_mode, "auto");

    // A typo is a client error, not a silent fallback.
    let typo_status = send(
        app.clone(),
        "PATCH",
        "/api/admin/settings",
        Some(&operator),
        Some(json!({"email_mode": "bananas"})),
    )
    .await
    .status();
    assert_eq!(
        typo_status,
        StatusCode::BAD_REQUEST,
        "unknown mode rejected"
    );

    // The failed patch must not have overwritten the good value.
    let after_typo: Value = body_json(
        send(
            app.clone(),
            "GET",
            "/api/admin/settings",
            Some(&operator),
            None,
        )
        .await,
    )
    .await;
    let after_typo_mode = after_typo["email_mode"].as_str().expect("email_mode");
    assert_eq!(after_typo_mode, "auto", "bad input changes nothing");

    // Leave no stored override behind for other test binaries sharing this
    // database: the boot configuration must decide for them.
    sqlx::query("DELETE FROM system_settings WHERE key = 'agent.mode'")
        .execute(&pg)
        .await
        .expect("clear override");
}
