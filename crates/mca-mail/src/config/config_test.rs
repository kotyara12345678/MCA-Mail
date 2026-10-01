//! Tests for `config/mod.rs`.

#![cfg(test)]

use super::*;
use crate::config::{
    AgentSettings, EmailMode, ImapSettings, LlmProviderKind, OutboundPolicy, Secret, SmtpSettings,
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
