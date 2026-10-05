use super::*;
use std::path::PathBuf;

fn settings() -> BackupSettings {
    BackupSettings {
        enabled: true,
        dir: std::env::temp_dir().join("mca-backups"),
        ..Default::default()
    }
}

#[test]
fn enabled_backup_validation_creates_its_directory() {
    let dir = std::env::temp_dir().join(format!("mca-backup-start-{}", uuid::Uuid::new_v4()));
    assert!(!dir.exists());
    assert!(check(&BackupSettings {
        dir: dir.clone(),
        ..settings()
    })
    .is_ok());
    assert!(dir.is_dir());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn partial_backup_environment_keeps_defaults_for_missing_fields() {
    let settings: BackupSettings = serde_json::from_value(serde_json::json!({
        "enabled": false
    }))
    .unwrap();
    assert!(!settings.enabled);
    assert_eq!(settings.interval_hours, 6);
    assert_eq!(settings.retention_days, 7);
}

#[test]
fn disabled_backups_ignore_invalid_fields() {
    let broken = BackupSettings {
        enabled: false,
        dir: PathBuf::new(),
        interval_hours: 0,
        retention_days: 0,
        max_size_mb: 0,
        ..Default::default()
    };
    assert!(check(&broken).is_ok());
}

#[test]
fn interval_and_limit_errors_are_rejected() {
    for broken in [
        BackupSettings {
            interval_hours: 0,
            ..settings()
        },
        BackupSettings {
            interval_hours: 8761,
            ..settings()
        },
        BackupSettings {
            retention_days: 0,
            ..settings()
        },
        BackupSettings {
            max_size_mb: 0,
            ..settings()
        },
        BackupSettings {
            command_timeout_seconds: 86_401,
            ..settings()
        },
        BackupSettings {
            retention_weeks: 53,
            ..settings()
        },
    ] {
        assert!(check(&broken).is_err());
    }
}

#[test]
fn relative_or_empty_directories_are_rejected() {
    for dir in [PathBuf::from("backups"), PathBuf::new()] {
        assert!(check(&BackupSettings { dir, ..settings() }).is_err());
    }
}
