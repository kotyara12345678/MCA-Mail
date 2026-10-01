//! Event helper tests: queue and system (migrations, DB, recovery) events.

use super::test_util::{await_marker, run};
use super::{bus, queue, system};

#[test]
fn queue_events_cover_poll_batch_and_workers() {
    let mut rx = bus::subscribe();
    run(|| {
        queue::poll_started("corp@");
        queue::poll_completed("corp@", 120, 3);
        queue::emails_fetched(3);
        queue::emails_inserted(2);
        queue::emails_skipped_duplicate(1);
        queue::batch_claimed(4);
        queue::batch_completed(4, 3, 1, 5000);
        queue::worker_started(uuid::Uuid::new_v4());
        queue::worker_completed(uuid::Uuid::new_v4(), 900);
        queue::worker_failed(uuid::Uuid::new_v4(), "queue-mark-wf", 900);
    });
    for name in [
        "poll_started",
        "poll_completed",
        "emails_fetched",
        "emails_inserted",
        "emails_skipped_duplicate",
        "batch_claimed",
        "batch_completed",
        "worker_started",
        "worker_completed",
    ] {
        let ev = await_marker(&mut rx, &format!("\"event\":\"{name}\""));
        assert_eq!(ev.event, name);
    }
    let ev = await_marker(&mut rx, "queue-mark-wf");
    assert_eq!(ev.event, "worker_failed");
}

#[test]
fn system_events_cover_migrations_db_and_recovery() {
    let mut rx = bus::subscribe();
    let email_id = uuid::Uuid::new_v4();
    run(|| {
        system::migration_started();
        system::migration_completed(35);
        system::database_connection_failed("pool_timed_out", "connect timeout");
        system::repository_error("claim_batch", "database", "sys-mark-repo");
        system::transaction_failed("upsert_many", "database", "deadlock");
        system::recovery_started();
        system::stuck_emails_found(2, 1);
        system::processing_recovered(email_id);
        system::recovery_completed(2, 1, 15);
    });
    for name in [
        "migration_started",
        "migration_completed",
        "database_connection_failed",
    ] {
        let ev = await_marker(&mut rx, &format!("\"event\":\"{name}\""));
        assert_eq!(ev.event, name);
    }
    let ev = await_marker(&mut rx, "sys-mark-repo");
    assert_eq!(ev.event, "repository_error");
    assert!(ev.json.contains("\"component\":\"repository\""));
    for name in [
        "transaction_failed",
        "recovery_started",
        "stuck_emails_found",
        "processing_recovered",
        "recovery_completed",
    ] {
        let ev = await_marker(&mut rx, &format!("\"event\":\"{name}\""));
        assert_eq!(ev.event, name);
    }
}
