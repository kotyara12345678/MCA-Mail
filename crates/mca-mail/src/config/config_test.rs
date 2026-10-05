//! Tests for `config/mod.rs`.

#![cfg(test)]

use super::*;
use crate::config::{
    AgentSettings, BackupSettings, EmailMode, ImapSettings, LlmProviderKind, MailMode,
    OutboundPolicy, Secret, SmtpSettings,
};

fn base() -> AppConfig {
    AppConfig {
        database: DatabaseSettings {
            url: "postgres://u:p@localhost:5432/db".into(),
            auto_migrate: true,
            ..Default::default()
        },
        llm: LlmSettings {
            disabled: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn mail_mode_defaults_to_read_only() {
    let cfg = base();
    assert_eq!(cfg.mail.mode, MailMode::ReadOnly);
    assert!(cfg.validate().is_ok());
}

#[test]
fn an_unknown_mail_mode_value_is_rejected() {
    assert!(MailMode::parse("write_everything").is_err());
    assert!(MailMode::parse("AUTO").is_err());
    assert_eq!(MailMode::parse("work").unwrap(), MailMode::ReadWrite);
}

#[test]
fn work_mode_still_validates_the_transport() {
    // `work` grants mailbox writes; it does not waive the existing safety rules.
    let cfg = AppConfig {
        mail: MailSettings {
            mode: MailMode::Work,
            ..Default::default()
        },
        ..base()
    };
    assert!(cfg.validate().is_ok(), "mock provider + work mode is fine");
    assert!(cfg.mail.mode.allows_mailbox_write());
}

#[test]
fn quarantine_folder_falls_back_to_the_special_use_role() {
    let cfg = base();
    assert_eq!(
        cfg.mail.quarantine_folder(),
        None,
        "no folder configured means the Spam role has to be resolved"
    );

    let cfg = AppConfig {
        mail: MailSettings {
            spam_folder: "  Junk  ".into(),
            ..Default::default()
        },
        ..base()
    };
    assert_eq!(cfg.mail.quarantine_folder(), Some("Junk"));
}

#[test]
fn sent_folder_falls_back_to_the_special_use_role() {
    let cfg = base();
    assert_eq!(
        cfg.mail.sent_folder(),
        None,
        "no folder configured means the Sent role has to be resolved"
    );

    let cfg = AppConfig {
        mail: MailSettings {
            sent_folder: "  Sent Items  ".into(),
            ..Default::default()
        },
        ..base()
    };
    assert_eq!(cfg.mail.sent_folder(), Some("Sent Items"));
}

#[test]
fn backup_settings_are_validated_at_startup() {
    let cfg = AppConfig {
        backup: BackupSettings {
            enabled: true,
            dir: std::path::PathBuf::from("relative/backups"),
            ..Default::default()
        },
        ..base()
    };
    assert!(
        cfg.validate().is_err(),
        "a relative BACKUP_DIR must be refused"
    );
}

#[test]
fn backups_work_while_read_only() {
    // Backups are independent of the mail mode: read-only must not disable them.
    let cfg = AppConfig {
        mail: MailSettings {
            mode: MailMode::ReadOnly,
            ..Default::default()
        },
        backup: BackupSettings {
            enabled: true,
            dir: std::env::temp_dir().join("mca-backups"),
            ..Default::default()
        },
        ..base()
    };
    assert!(cfg.validate().is_ok());
    assert!(cfg.backup.enabled);
    assert_eq!(cfg.mail.mode, MailMode::ReadOnly);
}

#[test]
fn defaults_validate_with_mock_providers() {
    let cfg = base();
    assert!(cfg.validate().is_ok());
    assert_eq!(cfg.security.email_mode, EmailMode::DryRun);
}

#[test]
fn missing_database_url_is_reported_by_name() {
    let cfg = AppConfig {
        database: DatabaseSettings::default(),
        ..base()
    };
    assert_eq!(cfg.validate().unwrap_err().field_hint(), "DATABASE_URL");
}

#[test]
fn non_postgres_url_is_rejected() {
    let cfg = AppConfig {
        database: DatabaseSettings {
            url: "mysql://localhost/db".into(),
            ..Default::default()
        },
        ..base()
    };
    assert!(cfg.validate().is_err());
}

#[test]
fn auto_send_without_auto_mode_is_refused() {
    let cfg = AppConfig {
        security: SecuritySettings {
            email_mode: EmailMode::Review,
            outbound: OutboundPolicy {
                auto_send: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..base()
    };
    assert_eq!(cfg.validate().unwrap_err().field_hint(), "EMAIL_AUTO_SEND");
}

#[test]
fn production_refuses_mock_mail_provider() {
    let cfg = AppConfig {
        app: AppSettings {
            env: "production".into(),
            ..Default::default()
        },
        ..base()
    };
    assert_eq!(cfg.validate().unwrap_err().field_hint(), "APP_ENV");
}

#[test]
fn production_imap_requires_credentials_and_tls() {
    let cfg = AppConfig {
        app: AppSettings {
            env: "production".into(),
            ..Default::default()
        },
        mail: MailSettings {
            provider: MailProviderKind::Imap,
            imap: ImapSettings {
                host: "imap.example.com".into(),
                tls: TlsMode::None,
                ..Default::default()
            },
            smtp: SmtpSettings {
                host: "smtp.example.com".into(),
                ..Default::default()
            },
            ..Default::default()
        },
        ..base()
    };
    assert_eq!(cfg.validate().unwrap_err().field_hint(), "MAIL_USERNAME");
}

#[test]
fn real_llm_requires_endpoint_model_and_key() {
    let cfg = AppConfig {
        llm: LlmSettings {
            provider: LlmProviderKind::OpenAiCompatible,
            ..Default::default()
        },
        ..base()
    };
    assert_eq!(cfg.validate().unwrap_err().field_hint(), "LLM_BASE_URL");
}

#[test]
fn zero_iteration_budget_is_rejected() {
    let cfg = AppConfig {
        agents: AgentSettings {
            max_iterations: 0,
            ..Default::default()
        },
        ..base()
    };
    assert_eq!(
        cfg.validate().unwrap_err().field_hint(),
        "AGENT_MAX_ITERATIONS"
    );
}

// --- \.env\ handling ----------------------------------------------------

fn dotenv_err(err: dotenvy::Error) -> Result<std::path::PathBuf, dotenvy::Error> {
    Err(err)
}

fn missing_env() -> Result<std::path::PathBuf, dotenvy::Error> {
    dotenv_err(dotenvy::Error::Io(std::io::Error::from(
        std::io::ErrorKind::NotFound,
    )))
}

fn unreadable_env() -> Result<std::path::PathBuf, dotenvy::Error> {
    dotenv_err(dotenvy::Error::Io(std::io::Error::from(
        std::io::ErrorKind::PermissionDenied,
    )))
}

#[test]
fn a_missing_env_file_is_not_an_error() {
    assert!(interpret_dotenv(missing_env()).is_ok());
}

#[test]
fn an_unreadable_env_file_stops_startup() {
    assert!(matches!(
        interpret_dotenv(unreadable_env()),
        Err(crate::error::ConfigError::EnvFile(_))
    ));
}

#[test]
fn an_unparseable_env_file_stops_startup() {
    let broken = dotenv_err(dotenvy::Error::LineParse("NOEQUALS".into(), 1));
    assert!(matches!(
        interpret_dotenv(broken),
        Err(crate::error::ConfigError::EnvFile(_))
    ));
}

#[test]
fn the_env_file_error_names_dotenv_not_a_config_key() {
    let err = interpret_dotenv(unreadable_env()).expect_err("must fail");
    assert_eq!(err.field_hint(), ".env");
}
