use mca_mail::domain::{EmailDraft, InboundMessage, RequirementScope};
use mca_mail::persistence;
use sqlx::PgPool;

use crate::support;

pub(super) async fn create_draft(
    pool: &PgPool,
    email_id: uuid::Uuid,
    msg: &InboundMessage,
) -> (uuid::Uuid, bool) {
    let draft = EmailDraft {
        id: uuid::Uuid::new_v4(),
        lead_id: Some(lead(pool).await),
        email_id: Some(email_id),
        in_reply_to: msg.internet_message_id.clone(),
        to_addresses: vec!["buyer@example.com".into()],
        cc_addresses: vec![],
        subject: "Re: Please send pricing".into(),
        body: "Here is our pricing.".into(),
        status: mca_mail::domain::DraftStatus::PendingApproval,
        idempotency_key: support::unique("read-only-draft"),
        suppression_reason: None,
        reviewed_by: None,
        reviewed_at: None,
        sent_at: None,
        provider_message_id: None,
        created_at: chrono::Utc::now(),
    };
    persistence::draft_repo::create(pool, &draft)
        .await
        .expect("create internal draft")
}

pub(super) async fn insert_email(pool: &PgPool, msg: &InboundMessage) -> uuid::Uuid {
    let subject = mca_mail::domain::normalize_subject(&msg.subject);
    let thread = persistence::thread_repo::ensure_thread(
        pool,
        &persistence::thread_repo::conversation_key(&[&msg.from], &subject),
        &subject,
        msg.internet_message_id.as_deref(),
    )
    .await
    .expect("thread");
    persistence::email_repo::insert_inbound(pool, thread, "INBOX", msg, None)
        .await
        .expect("insert")
        .email_id()
}

pub(super) async fn lead(pool: &PgPool) -> uuid::Uuid {
    persistence::lead_repo::ensure(
        pool,
        &support::unique("read-only-lead"),
        "buyer@example.com",
        None,
        None,
        RequirementScope::Transport,
    )
    .await
    .expect("lead")
    .0
}
