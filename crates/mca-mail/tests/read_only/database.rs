use mca_mail::config::MailMode;
use mca_mail::domain::{EmailCategory, SpamVerdict};
use mca_mail::mail::{MailProvider, MailboxWriter, MockMailProvider};
use mca_mail::persistence;

use super::fixtures::{is_refusal, message, outbound};
use crate::support;

#[path = "database_helpers.rs"]
mod helpers;

#[tokio::test]
async fn read_only_stores_the_email_and_creates_a_draft() {
    let Some(pool) = support::pool().await else {
        eprintln!("skipped: set MCA_TEST_DATABASE_URL");
        return;
    };
    let msg = message(
        "Please send pricing",
        "buyer@example.com",
        "Quote for 40 units please.",
    );
    let provider = MockMailProvider::with_messages(vec![msg.clone()]);
    assert_eq!(provider.guard().mode(), MailMode::ReadOnly);
    assert_eq!(provider.fetch_new().await.unwrap().len(), 1);

    let email_id = helpers::insert_email(&pool, &msg).await;
    persistence::email_repo::set_classification(
        &pool,
        email_id,
        EmailCategory::NewLead,
        SpamVerdict::NotSpam,
    )
    .await
    .expect("classification write");
    persistence::email_repo::attach_lead(&pool, email_id, helpers::lead(&pool).await)
        .await
        .expect("attach lead");
    let (draft_id, created) = helpers::create_draft(&pool, email_id, &msg).await;
    assert!(created);
    assert_eq!(
        persistence::email_repo::get(&pool, email_id)
            .await
            .unwrap()
            .id,
        email_id
    );
    assert_eq!(
        persistence::draft_repo::get(&pool, draft_id)
            .await
            .unwrap()
            .body,
        "Here is our pricing."
    );
    assert!(is_refusal(
        MailboxWriter::append_draft(&provider, &outbound()).await
    ));
}
