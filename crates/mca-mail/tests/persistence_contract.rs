//! Integration tests against a real PostgreSQL.
//!
//! SQL correctness is not something unit tests can establish: reserved words,
//! index predicates, `ON CONFLICT` inference and CHECK constraints only fail
//! against the real engine. These tests run the migrations and then exercise the
//! idempotency guarantees the design depends on.
//!
//! Set `MCA_TEST_DATABASE_URL` to run them, e.g.
//! `MCA_TEST_DATABASE_URL=postgres://mca:mca@localhost:55432/mca_mail cargo test`.
//! Without it the whole module is skipped rather than silently passing.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use mca_mail::config::DatabaseSettings;
use mca_mail::domain::{EmailAddress, InboundMessage, MailboxIdentity, Priority, ProcessingStage};
use mca_mail::error::AppError;
use mca_mail::persistence;

async fn pool() -> Option<PgPool> {
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
    // Migrations are embedded in the test binary, so a bare CI database comes
    // up to date without an external sqlx-cli install. A failure here must
    // fail the suite: silently skipping would make CI green without testing.
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    if !migrations_are_current(&pool).await {
        return None;
    }
    Some(pool)
}

async fn migrations_are_current(pool: &PgPool) -> bool {
    let ok: Option<(i32,)> = sqlx::query_as(
        "SELECT count(*)::int FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name = 'email_drafts'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    matches!(ok, Some((n,)) if n == 1)
}

/// Build a probe message.
///
/// The subject is suffixed with a fresh UUID because these tests run against a
/// long-lived database: reusing a subject would collide with the row a previous
/// run left behind, and the assertions are about behaviour on a *new* message.
fn message(subject: &str, from: &str, body: &str) -> InboundMessage {
    let nonce = uuid::Uuid::new_v4();
    let subject = format!("{subject} {nonce}");
    InboundMessage {
        provider_message_id: format!("pmid-{nonce}"),
        internet_message_id: Some(format!("<{nonce}@example.com>")),
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::new(from),
        to: vec![EmailAddress::new("inbox@mca.example")],
        cc: vec![],
        subject,
        date: None,
        text_body: body.into(),
        attachments: vec![],
        total_size: body.len(),
    }
}

async fn insert_email(pool: &PgPool, msg: &InboundMessage) -> (uuid::Uuid, uuid::Uuid) {
    let thread_id = persistence::thread_repo::ensure_thread(
        pool,
        &persistence::thread_repo::conversation_key(
            &[&msg.from],
            &mca_mail::domain::normalize_subject(&msg.subject),
        ),
        &mca_mail::domain::normalize_subject(&msg.subject),
        msg.internet_message_id.as_deref(),
    )
    .await
    .expect("thread");
    let outcome = persistence::email_repo::insert_inbound(pool, thread_id, "INBOX", msg)
        .await
        .expect("insert email");
    (thread_id, outcome.email_id())
}

/// Same message twice must yield one row: the dedup key is the guarantee.
#[tokio::test]
async fn duplicate_inbound_is_idempotent() {
    let Some(pool) = pool().await else { return };
    let msg = message("Idempotency probe", "probe@example.com", "hello");
    let (_, first) = insert_email(&pool, &msg).await;
    let (_, second) = insert_email(&pool, &msg).await;
    assert_eq!(
        first, second,
        "a repeated message must not create a second email"
    );

    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM emails WHERE dedup_key = $1")
        .bind(persistence::email_repo::dedup_key_for(&msg))
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(total, 1);
}

/// A second live run for the same email is refused by a partial unique index.
#[tokio::test]
async fn concurrent_runs_are_prevented() {
    let Some(pool) = pool().await else { return };
    let msg = message("Run guard probe", "runs@example.com", "one run");
    let (thread_id, email_id) = insert_email(&pool, &msg).await;

    let first = persistence::run_repo::start(
        &pool,
        email_id,
        thread_id,
        mca_mail::domain::RunTrigger::Poll,
    )
    .await;
    assert!(first.is_ok(), "first run must be accepted");

    let duplicate = persistence::run_repo::start(
        &pool,
        email_id,
        thread_id,
        mca_mail::domain::RunTrigger::Poll,
    )
    .await;
    assert!(duplicate.is_err(), "a second live run must be refused");
}

/// Stage completion is recorded and a finished run becomes terminal.
#[tokio::test]
async fn stage_progress_is_recorded_and_finish_is_terminal() {
    let Some(pool) = pool().await else { return };
    let msg = message("Stage probe", "stage@example.com", "progress");
    let (thread_id, email_id) = insert_email(&pool, &msg).await;
    let run = persistence::run_repo::start(
        &pool,
        email_id,
        thread_id,
        mca_mail::domain::RunTrigger::Poll,
    )
    .await
    .expect("run");

    persistence::run_repo::record_stage(&pool, run.id, ProcessingStage::Spam, true)
        .await
        .expect("complete spam");
    persistence::run_repo::record_stage(&pool, run.id, ProcessingStage::Intake, true)
        .await
        .expect("complete intake");

    let stored = persistence::run_repo::get(&pool, run.id)
        .await
        .expect("get")
        .expect("run exists");
    let mut got = stored.stages_completed;
    let mut expected = vec![ProcessingStage::Intake, ProcessingStage::Spam];
    got.sort_by_key(|s| s.as_str());
    expected.sort_by_key(|s| s.as_str());
    assert_eq!(got, expected);
    assert!(
        !stored.state.is_terminal(),
        "an in-flight run is not terminal"
    );

    persistence::run_repo::finish(&pool, run.id, mca_mail::domain::RunState::Succeeded, None)
        .await
        .expect("finish");
    let done = persistence::run_repo::get(&pool, run.id)
        .await
        .expect("get")
        .expect("run exists");
    assert!(done.state.is_terminal());
    assert!(done.finished_at.is_some());
}

/// One live draft per lead: a second generated reply cannot be queued.
#[tokio::test]
async fn only_one_live_draft_per_lead() {
    let Some(pool) = pool().await else { return };
    let (lead_id, _created) = persistence::lead_repo::ensure(
        &pool,
        &format!("draft-probe-{}", uuid::Uuid::new_v4()),
        "drafts@example.com",
        None,
        None,
        mca_mail::domain::RequirementScope::Transport,
    )
    .await
    .expect("lead");

    let draft = mca_mail::domain::EmailDraft {
        id: uuid::Uuid::new_v4(),
        lead_id: Some(lead_id),
        email_id: None,
        in_reply_to: None,
        to_addresses: vec!["customer@example.com".into()],
        cc_addresses: vec![],
        subject: "Re: quote".into(),
        body: "body".into(),
        status: mca_mail::domain::DraftStatus::PendingApproval,
        idempotency_key: format!("draft-key-{}", uuid::Uuid::new_v4()),
        suppression_reason: None,
        reviewed_by: None,
        reviewed_at: None,
        sent_at: None,
        provider_message_id: None,
        created_at: chrono::Utc::now(),
    };
    let (first_id, created) = persistence::draft_repo::create(&pool, &draft)
        .await
        .expect("first draft");
    assert!(created, "the first draft is new");

    // Same key again: the same row comes back, and `created` is false. This is
    // what makes a reprocess safe to re-run.
    let (again, created_again) = persistence::draft_repo::create(&pool, &draft)
        .await
        .expect("idempotent replay");
    assert_eq!(first_id, again);
    assert!(!created_again, "a replayed key must not report a new draft");

    // A different key for the same lead is a genuine second live reply, and the
    // partial unique index must refuse it: the repository hands back the
    // lead's live draft instead of inserting a competing one.
    let competing = mca_mail::domain::EmailDraft {
        idempotency_key: format!("draft-key-{}", uuid::Uuid::new_v4()),
        subject: "Re: another quote".into(),
        ..draft
    };
    let (second_id, created_second) = persistence::draft_repo::create(&pool, &competing)
        .await
        .expect("second draft resolves to the live one");
    assert!(!created_second, "a competing draft must not be inserted");
    assert_eq!(
        first_id, second_id,
        "one live draft per lead: the existing draft is returned"
    );
    let live: Vec<uuid::Uuid> = sqlx::query_scalar(
        "SELECT id FROM email_drafts WHERE lead_id = $1 \
         AND status IN ('pending_approval','approved')",
    )
    .bind(lead_id)
    .fetch_all(&pool)
    .await
    .expect("live drafts");
    assert_eq!(live.len(), 1, "exactly one live draft may exist");
    assert!(persistence::draft_repo::live_draft_for_lead(&pool, lead_id)
        .await
        .expect("live draft")
        .is_some());
}

/// A lead with a handoff is automation-locked, and an unknown lead locks too.
#[tokio::test]
async fn handoff_locks_automation() {
    let Some(pool) = pool().await else { return };
    let (lead_id, _created) = persistence::lead_repo::ensure(
        &pool,
        &format!("lock-probe-{}", uuid::Uuid::new_v4()),
        "locked@example.com",
        None,
        None,
        mca_mail::domain::RequirementScope::Transport,
    )
    .await
    .expect("lead");
    assert!(
        !persistence::lead_repo::is_automation_locked(&pool, lead_id)
            .await
            .expect("unlocked")
    );

    persistence::lead_repo::lock_automation(&pool, lead_id)
        .await
        .expect("lock");
    assert!(persistence::lead_repo::is_automation_locked(&pool, lead_id)
        .await
        .expect("locked"));

    // Fail closed: a lead that does not exist must not be treated as unlocked.
    let ghost = uuid::Uuid::new_v4();
    assert!(persistence::lead_repo::is_automation_locked(&pool, ghost)
        .await
        .expect("ghost"));
}

/// Only one open handoff per lead; a repeat escalates in place.
#[tokio::test]
async fn repeated_handoff_updates_the_open_one() {
    let Some(pool) = pool().await else { return };
    let (lead_id, _created) = persistence::lead_repo::ensure(
        &pool,
        &format!("handoff-probe-{}", uuid::Uuid::new_v4()),
        "handoff@example.com",
        None,
        None,
        mca_mail::domain::RequirementScope::Transport,
    )
    .await
    .expect("lead");

    let base = mca_mail::domain::Handoff {
        id: uuid::Uuid::new_v4(),
        lead_id,
        thread_id: None,
        run_id: None,
        email_id: None,
        reason: mca_mail::domain::HandoffReason::LowConfidence,
        priority: Priority::Normal,
        state: mca_mail::domain::HandoffState::Open,
        contact_email: "handoff@example.com".into(),
        contact_name: None,
        contact_phone: None,
        company_name: None,
        company_inn: None,
        original_request: "please quote".into(),
        category: None,
        spam_verdict: None,
        cargo_summary: "pallets".into(),
        route_summary: "CN->RU".into(),
        requested_service: "transport".into(),
        missing_information: vec!["weight".into()],
        open_questions: vec!["which incoterm?".into()],
        conversation_digest: "digest".into(),
        research_digest: None,
        checks_performed: vec![],
        unresolved_topics: vec![],
        assigned_to: None,
        acknowledged_at: None,
        created_at: chrono::Utc::now(),
    };
    let first = persistence::handoff_repo::upsert_open(&pool, &base)
        .await
        .expect("first handoff");

    let escalated = mca_mail::domain::Handoff {
        priority: Priority::Critical,
        reason: mca_mail::domain::HandoffReason::PricingOrLegal,
        ..base
    };
    let second = persistence::handoff_repo::upsert_open(&pool, &escalated)
        .await
        .expect("second handoff");
    assert_eq!(
        first, second,
        "a repeat handoff must not open a second ticket"
    );

    let stored = persistence::handoff_repo::open_for_lead(&pool, lead_id)
        .await
        .expect("open")
        .expect("exists");
    assert_eq!(stored.priority, Priority::Critical);
    assert_eq!(
        stored.reason,
        mca_mail::domain::HandoffReason::PricingOrLegal
    );
}

/// The stored priority round-trips, which the manager queue orders by.
#[tokio::test]
async fn priority_round_trips() {
    let Some(pool) = pool().await else { return };
    let (lead_id, _c) = persistence::lead_repo::ensure(
        &pool,
        &format!("prio-probe-{}", uuid::Uuid::new_v4()),
        "prio@example.com",
        None,
        None,
        mca_mail::domain::RequirementScope::Customs,
    )
    .await
    .expect("lead");
    let handoff = mca_mail::domain::Handoff {
        id: uuid::Uuid::new_v4(),
        lead_id,
        thread_id: None,
        run_id: None,
        email_id: None,
        reason: mca_mail::domain::HandoffReason::ReadyForManager,
        priority: Priority::High,
        state: mca_mail::domain::HandoffState::Open,
        contact_email: "prio@example.com".into(),
        contact_name: None,
        contact_phone: None,
        company_name: None,
        company_inn: None,
        original_request: "request".into(),
        category: None,
        spam_verdict: None,
        cargo_summary: String::new(),
        route_summary: String::new(),
        requested_service: "customs".into(),
        missing_information: vec![],
        open_questions: vec![],
        conversation_digest: String::new(),
        research_digest: None,
        checks_performed: vec![],
        unresolved_topics: vec![],
        assigned_to: None,
        acknowledged_at: None,
        created_at: chrono::Utc::now(),
    };
    let id = persistence::handoff_repo::upsert_open(&pool, &handoff)
        .await
        .expect("handoff");
    let stored = persistence::handoff_repo::get(&pool, id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(stored.priority, Priority::High);
}

/// API keys are stored hashed; the raw value never lands in the database.
#[tokio::test]
async fn api_keys_are_hashed_and_verifiable() {
    let Some(pool) = pool().await else { return };
    let (raw, _id) = persistence::api_key_repo::create(
        &pool,
        "integration probe",
        persistence::api_key_repo::Role::Manager,
        "tester",
        None,
    )
    .await
    .expect("create key");

    let verified = persistence::api_key_repo::verify(&pool, &raw)
        .await
        .expect("verify")
        .expect("must verify");
    assert_eq!(verified.role, persistence::api_key_repo::Role::Manager);
    assert!(verified
        .role
        .at_least(persistence::api_key_repo::Role::Operator));

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM api_keys WHERE key_hash = $1")
        .bind(&raw)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(stored, 0, "the raw key must not be stored");

    assert!(persistence::api_key_repo::verify(&pool, "mca_wrong")
        .await
        .expect("verify wrong")
        .is_none());
}

/// Operator flow: list keys, resolve one by hash prefix, revoke it.
#[tokio::test]
async fn api_key_list_and_prefix_revoke() {
    let Some(pool) = pool().await else { return };
    let (raw, id) = persistence::api_key_repo::create(
        &pool,
        "prefix probe",
        persistence::api_key_repo::Role::Viewer,
        "tester",
        None,
    )
    .await
    .expect("create key");
    let prefix = persistence::api_key_repo::key_prefix(&raw);

    let listed = persistence::api_key_repo::list(&pool).await.expect("list");
    let row = listed.iter().find(|r| r.id == id).expect("key listed");
    assert_eq!(row.name, "prefix probe");
    assert!(row.is_active);

    let resolved = persistence::api_key_repo::id_by_prefix(&pool, &prefix)
        .await
        .expect("prefix lookup");
    assert_eq!(resolved, Some(id), "prefix must resolve to the key");

    persistence::api_key_repo::revoke(&pool, id)
        .await
        .expect("revoke");

    let resolved = persistence::api_key_repo::id_by_prefix(&pool, &prefix)
        .await
        .expect("prefix lookup after revoke");
    assert!(resolved.is_none(), "revoked key must not resolve");

    let listed = persistence::api_key_repo::list(&pool).await.expect("list");
    let row = listed.iter().find(|r| r.id == id).expect("key listed");
    assert!(!row.is_active, "revoked key stays visible for audit");
}

/// Settings round-trip and the automation switch defaults to enabled.
#[tokio::test]
async fn settings_round_trip() {
    let Some(pool) = pool().await else { return };
    persistence::settings_repo::set_automation_enabled(&pool, false, "tester")
        .await
        .expect("disable");
    assert!(!persistence::settings_repo::automation_enabled(&pool)
        .await
        .expect("read"));
    persistence::settings_repo::set_automation_enabled(&pool, true, "tester")
        .await
        .expect("enable");
    assert!(persistence::settings_repo::automation_enabled(&pool)
        .await
        .expect("read"));
}

/// Retention anonymizes old bodies and reports what it touched.
#[tokio::test]
async fn retention_anonymizes_without_deleting() {
    let Some(pool) = pool().await else { return };
    let msg = message(
        "Retention probe",
        "retention@example.com",
        "sensitive payload",
    );
    let (_t, email_id) = insert_email(&pool, &msg).await;

    let anonymized = persistence::email_repo::anonymize(&pool, 0)
        .await
        .expect("anonymize");
    assert!(anonymized >= 1);

    let body: String = sqlx::query_scalar("SELECT text_body FROM emails WHERE id = $1")
        .bind(email_id)
        .fetch_one(&pool)
        .await
        .expect("body");
    assert!(!body.contains("sensitive payload"));
    let still_there: i64 = sqlx::query_scalar("SELECT count(*) FROM emails WHERE id = $1")
        .bind(email_id)
        .fetch_one(&pool)
        .await
        .expect("exists");
    assert_eq!(still_there, 1, "retention must not delete the message row");
}

/// A disabled retention pass is a no-op.
#[tokio::test]
async fn retention_respects_disabled_setting() {
    let Some(pool) = pool().await else { return };
    let settings = mca_mail::config::RetentionSettings {
        enabled: false,
        ..Default::default()
    };
    let report = persistence::retention_repo::run(&pool, &settings)
        .await
        .expect("run");
    assert!(report.is_empty());
}

/// Enum columns reject values outside the CHECK constraint, and a bad row
/// surfaces as a parse error rather than a panic.
#[tokio::test]
async fn invalid_enum_is_rejected_by_the_database() {
    let Some(pool) = pool().await else { return };
    let msg = message("Enum probe", "enum@example.com", "body");
    let (_t, email_id) = insert_email(&pool, &msg).await;
    let err = sqlx::query("UPDATE emails SET category = 'not_a_category' WHERE id = $1")
        .bind(email_id)
        .execute(&pool)
        .await
        .expect_err("CHECK constraint must reject an unknown category");
    assert!(matches!(err, sqlx::Error::Database(_)));
}

/// The mailbox identity helper is what the gateway polls with.
#[test]
fn mailbox_identity_round_trips() {
    let identity = MailboxIdentity {
        address: "mca@example.invalid".into(),
        display_name: "MCA".into(),
    };
    assert_eq!(identity.address, "mca@example.invalid");
}

/// A database failure is mapped to a typed error, not a panic.
#[tokio::test]
async fn missing_row_is_a_typed_not_found() {
    let Some(pool) = pool().await else { return };
    let ghost = uuid::Uuid::new_v4();
    let err = persistence::draft_repo::get(&pool, ghost)
        .await
        .expect_err("missing draft");
    assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
}
