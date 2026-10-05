//! Unified configuration layer.
//!
//! Sources, lowest priority first: built-in defaults, an optional TOML file,
//! then flat environment variables (and an optional `.env` file).
//!
//! Every setting has a default, so the service boots with the mock mail
//! provider, a mock LLM and `dry_run` mode. Anything that would be unsafe in
//! that combination is caught by [`AppConfig::validate`], which reports the
//! offending key by name and never echoes a secret value.

mod agents;
mod backup;
mod env;
mod env_tree;
mod llm;
mod mail_mode;
mod secret;
mod security;
mod settings;
mod summary;
mod validate;

#[cfg(test)]
#[path = "config_test.rs"]
mod tests;

pub use agents::{AgentSettings, ResearchSettings, RetentionSettings, ToolSettings};
pub use backup::BackupSettings;
pub use llm::{LlmProviderKind, LlmSettings, ModelPrice, ModelRouting};
pub use mail_mode::MailMode;
pub use secret::Secret;
pub use security::{
    AttachmentPolicy, ContextPolicy, EmailMode, InboundPolicy, ManagerCardSettings, OutboundPolicy,
};
pub use settings::{
    ApiSettings, AppSettings, DatabaseSettings, IdleSettings, ImapSettings, LogFormat,
    MailProviderKind, MailSettings, SecuritySettings, SmtpSettings, TlsMode,
};

use figment::providers::{Format, Serialized, Toml};
use figment::{Figment, Profile};
use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

/// Root configuration object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub app: AppSettings,
    pub api: ApiSettings,
    pub backup: BackupSettings,
    pub database: DatabaseSettings,
    pub mail: MailSettings,
    pub llm: LlmSettings,
    pub agents: AgentSettings,
    pub security: SecuritySettings,
    pub research: ResearchSettings,
    pub retention: RetentionSettings,
}

impl AppConfig {
    /// Load from defaults + optional TOML + environment.
    pub fn load() -> Result<Self, ConfigError> {
        load_dotenv()?;
        Self::from_sources("config/default.toml", "config/local.toml")
    }

    /// Load with explicit file paths, used by tests and the CLI.
    pub fn from_files(defaults: &str, local: &str) -> Result<Self, ConfigError> {
        load_dotenv()?;
        Self::from_sources(defaults, local)
    }

    pub fn from_sources(defaults: &str, local: &str) -> Result<Self, ConfigError> {
        let figment = Figment::new()
            .merge(Toml::file(defaults))
            .merge(Toml::file(local));

        // Build a nested JSON tree from the flat environment variables using the
        // declared mapping. A nested path is set one component at a time, so a
        // variable like `LLM_MODEL` never overwrites the whole `llm` struct and
        // the remaining fields keep their defaults.
        let environment = env_tree::build();
        let figment = figment.merge(Serialized::from(environment, Profile::Default));

        figment
            .extract()
            .map_err(|e| ConfigError::Load(summary::describe(&e.to_string())))
    }

    /// Cross-field checks that no single field can express.
    ///
    /// The goal is to fail at startup with a clear message rather than to let a
    /// half-configured deployment reach the point of sending real mail.
    pub fn validate(&self) -> Result<(), ConfigError> {
        validate::run(self)
    }

    /// Non-secret summary for the startup log and `/api/v1/settings`.
    pub fn redacted_summary(&self) -> serde_json::Value {
        summary::redacted(self)
    }
}

/// Load `.env` when there is one.
///
/// A missing file is normal in production, where the orchestrator injects real
/// environment variables, so that case stays silent. A `.env` that exists but
/// cannot be read or parsed is not: continuing would start the service with
/// whatever subset of the variables happened to load, which is how a pilot
/// ends up pointed at the wrong mailbox.
fn load_dotenv() -> Result<(), ConfigError> {
    interpret_dotenv(dotenvy::dotenv())
}

/// Split out so the decision — not the filesystem — is what gets tested.
fn interpret_dotenv(found: Result<std::path::PathBuf, dotenvy::Error>) -> Result<(), ConfigError> {
    match found {
        Ok(_) => Ok(()),
        Err(err) if err.not_found() => Ok(()),
        Err(err) => Err(ConfigError::EnvFile(err.to_string())),
    }
}
