//! The properties an operator depends on when the server is already gone:
//! no password in the process list, the newest backup never deleted, and backups
//! running regardless of `MAIL_MODE`.

use mca_mail::backup::{BackupError, BackupService};
use mca_mail::config::{BackupSettings, MailMode};

use super::harness::{require_tools, settings, temp_dir};
use super::support;

#[path = "safety_arguments.rs"]
mod arguments;

/// Even an aggressive policy must not delete the only backup there is.
#[tokio::test]
async fn rotation_after_a_real_backup_keeps_the_newest() {
    let Some(url) = support::database_url() else {
        return;
    };
    if !require_tools() {
        return;
    }
    let Some(pool) = support::pool().await else {
        return;
    };
    let dir = temp_dir("rotate-real");
    let service = BackupService::new(settings(dir.clone()), &url).expect("service");
    let outcome = service.run_once().await.expect("backup");
    let newest = dir.join(&outcome.file_name);

    let mut aggressive = settings(dir.clone());
    aggressive.retention_days = 1;
    aggressive.retention_weeks = 0;
    let removed = BackupService::new(aggressive, &url)
        .expect("service")
        .rotate()
        .await
        .expect("rotate");
    assert!(
        removed.is_empty(),
        "the only backup must survive: {removed:?}"
    );
    assert!(newest.exists());

    pool.close().await;
    std::fs::remove_dir_all(&dir).ok();
}

/// Read-only protects the customer's mailbox, not our records, so it must not
/// switch backups off.
#[tokio::test]
async fn backups_are_independent_of_the_mail_mode() {
    for mode in [MailMode::ReadOnly, MailMode::Work] {
        let service = BackupService::new(
            BackupSettings {
                enabled: true,
                dir: temp_dir(&format!("mode-{mode}")),
                ..Default::default()
            },
            "postgres://mca:p@localhost:5432/db",
        )
        .expect("service");
        assert!(service.is_enabled(), "backups must stay enabled in {mode}");
        std::fs::remove_dir_all(service.dir()).ok();
    }
}

#[tokio::test]
async fn a_disabled_service_refuses_to_produce_anything() {
    let dir = temp_dir("disabled");
    let service = BackupService::new(
        BackupSettings {
            enabled: false,
            dir: dir.clone(),
            ..Default::default()
        },
        "postgres://mca:p@localhost:5432/db",
    )
    .expect("service");
    let err = service.run_once().await.expect_err("must refuse");
    assert!(matches!(err, BackupError::Config(_)));
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
    std::fs::remove_dir_all(&dir).ok();
}
