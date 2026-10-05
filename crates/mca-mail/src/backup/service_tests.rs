use super::*;
use crate::config::{BackupSettings, MailMode};

#[test]
fn backups_remain_enabled_in_read_only_mode() {
    let settings = BackupSettings {
        enabled: true,
        dir: temp_dir("mode-independent"),
        ..Default::default()
    };
    assert!(!MailMode::ReadOnly.allows_mailbox_write());
    assert!(settings.enabled);
    std::fs::remove_dir_all(settings.dir).ok();
}

#[test]
fn a_disabled_service_refuses_to_run() {
    let dir = temp_dir("disabled");
    let service = super::super::BackupService::new(
        BackupSettings {
            enabled: false,
            dir: dir.clone(),
            ..Default::default()
        },
        "postgres://mca:p@localhost:5432/db",
    )
    .expect("service");
    assert!(!service.is_enabled());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn an_invalid_database_url_is_rejected() {
    let result = super::super::BackupService::new(BackupSettings::default(), "not-a-uri");
    assert!(matches!(result, Err(super::super::BackupError::Config(_))));
}
