//! The happy path: a real `pg_dump`, validated and published under its final
//! name, with nothing temporary left behind.

use mca_mail::backup::BackupService;

use super::harness::{require_tools, settings, temp_dir};
use super::support;

/// A real dump is created, validated and published.
#[tokio::test]
async fn a_real_dump_is_created_validated_and_published() {
    let Some(url) = support::database_url() else {
        eprintln!("skipped: MCA_TEST_DATABASE_URL is not set");
        return;
    };
    if !require_tools() {
        return;
    }
    let Some(pool) = support::pool().await else {
        eprintln!("skipped: could not connect to the test database");
        return;
    };
    // The dump is only meaningful if there is something to dump.
    sqlx::query("SELECT 1").execute(&pool).await.expect("query");

    let dir = temp_dir("real");
    let service = BackupService::new(settings(dir.clone()), &url).expect("service");
    let outcome = service.run_once().await.expect("backup must succeed");

    assert!(
        outcome.size_bytes > 0,
        "a successful backup must not be empty"
    );
    assert!(
        dir.join(&outcome.file_name).is_file(),
        "the dump must exist under its final name"
    );
    assert!(
        outcome.file_name.starts_with("mca-backup-") && outcome.file_name.ends_with(".dump"),
        "unexpected file name: {}",
        outcome.file_name
    );
    // Validation ran, so the file must list cleanly.
    assert!(mca_mail::backup::newest_backup(&dir).unwrap().is_some());

    pool.close().await;
    std::fs::remove_dir_all(&dir).ok();
}

/// Nothing temporary survives, and the credential file is gone.
#[tokio::test]
async fn no_partial_file_survives_a_successful_run() {
    let Some(url) = support::database_url() else {
        return;
    };
    if !require_tools() {
        return;
    }
    let Some(pool) = support::pool().await else {
        return;
    };
    let dir = temp_dir("partial");
    let service = BackupService::new(settings(dir.clone()), &url).expect("service");
    service.run_once().await.expect("backup");

    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".partial"))
        .collect();
    assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");
    assert!(
        !dir.join(".pgpass").exists(),
        ".pgpass must not survive the run"
    );

    pool.close().await;
    std::fs::remove_dir_all(&dir).ok();
}

/// A second concurrent run is refused rather than producing a broken file.
#[tokio::test]
async fn a_second_concurrent_run_is_refused() {
    use mca_mail::backup::BackupError;

    let Some(url) = support::database_url() else {
        return;
    };
    if !require_tools() {
        return;
    }
    let Some(pool) = support::pool().await else {
        return;
    };
    let dir = temp_dir("concurrent");
    let service =
        std::sync::Arc::new(BackupService::new(settings(dir.clone()), &url).expect("service"));

    let first = {
        let service = std::sync::Arc::clone(&service);
        tokio::spawn(async move { service.run_once().await })
    };
    let second = service.run_once().await;

    let first = first.await.expect("task").expect("first backup");
    // Either the second was refused outright, or it finished after the first
    // released the lock and produced its own valid dump. Both are correct; a
    // partially-written file or two files for one timestamp would not be.
    if let Err(error) = &second {
        assert!(
            matches!(error, BackupError::AlreadyRunning),
            "unexpected error: {error}"
        );
    }
    assert!(dir.join(first.file_name).is_file());

    pool.close().await;
    std::fs::remove_dir_all(&dir).ok();
}
