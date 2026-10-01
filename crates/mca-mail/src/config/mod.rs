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
mod env;
mod llm;
mod secret;
mod security;
mod settings;
mod summary;
mod validate;

#[cfg(test)]
#[path = "config_test.rs"]
mod tests;

pub use agents::{AgentSettings, ResearchSettings, RetentionSettings, ToolSettings};
pub use llm::{LlmProviderKind, LlmSettings, ModelPrice, ModelRouting};
pub use secret::Secret;
pub use security::{AttachmentPolicy, ContextPolicy, EmailMode, InboundPolicy, OutboundPolicy};
pub use settings::{
    ApiSettings, AppSettings, DatabaseSettings, ImapSettings, LogFormat, MailProviderKind,
    MailSettings, SecuritySettings, SmtpSettings, TlsMode,
};

use figment::providers::{Format, Serialized, Toml};
use figment::{Figment, Profile};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::error::ConfigError;

/// Root configuration object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub app: AppSettings,
    pub api: ApiSettings,
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
        // A missing `.env` is normal in production, where the orchestrator
        // injects real environment variables.
        let _ = dotenvy::dotenv();
        Self::from_sources("config/default.toml", "config/local.toml")
    }

    /// Load with explicit file paths, used by tests and the CLI.
    pub fn from_files(defaults: &str, local: &str) -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
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
        let env_tree = build_env_tree();
        let figment = figment.merge(Serialized::from(env_tree, Profile::Default));

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

/// Build a nested JSON tree from environment variables.
///
/// Only variables declared in the mapping tables are collected. Each value is
/// parsed as JSON when possible (`true`, `123`, `"text"`) and otherwise kept
/// as a string, so figment can coerce it to the target field type.
fn build_env_tree() -> JsonValue {
    let mut root = serde_json::Map::new();

    for (flat, nested) in env::all_pairs() {
        let Some(raw) = std::env::var(flat).ok() else {
            continue;
        };
        if raw.trim().is_empty() {
            continue;
        }
        let value = parse_env_value(&raw);
        insert_path(&mut root, nested, value);
    }

    JsonValue::Object(root)
}

fn parse_env_value(raw: &str) -> JsonValue {
    let trimmed = raw.trim();
    // Treat booleans, integers and floats as JSON literals.
    if trimmed.eq_ignore_ascii_case("true") {
        return JsonValue::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return JsonValue::Bool(false);
    }
    if let Ok(n) = trimmed.parse::<i64>() {
        return JsonValue::Number(n.into());
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return JsonValue::Number(n);
        }
    }
    JsonValue::String(raw.to_string())
}

fn insert_path(root: &mut serde_json::Map<String, JsonValue>, path: &str, value: JsonValue) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return;
    }
    let mut current = root;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            current.insert(part.to_string(), value);
            return;
        }
        let entry = current
            .entry(part.to_string())
            .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = JsonValue::Object(serde_json::Map::new());
        }
        current = entry.as_object_mut().unwrap();
    }
}
