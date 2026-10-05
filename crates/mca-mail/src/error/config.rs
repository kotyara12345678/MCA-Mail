//! Configuration problems. Rendered without ever echoing a secret value.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{0}")]
    Missing(String),
    #[error("invalid value for {field}: {reason}")]
    Invalid { field: String, reason: String },
    #[error("could not load configuration: {0}")]
    Load(String),
    #[error("could not read .env: {0}")]
    EnvFile(String),
    #[error("unsafe configuration in {mode} mode: {reason}")]
    Unsafe { mode: String, reason: String },
}

impl ConfigError {
    /// Path of the offending key, used by the bootstrap message and tests.
    pub fn field_hint(&self) -> String {
        match self {
            ConfigError::Missing(f) => f.clone(),
            ConfigError::Invalid { field, .. } => field.clone(),
            ConfigError::Load(f) => f.clone(),
            ConfigError::EnvFile(_) => ".env".to_string(),
            ConfigError::Unsafe { mode, .. } => mode.clone(),
        }
    }
}
