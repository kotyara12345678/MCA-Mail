//! Top-level error type.
//!
//! Every fallible boundary returns [`AppError`] so the API layer can map
//! failures onto stable HTTP responses without string matching.

mod agent;
mod classify;
mod config;
mod llm;
mod mail;
mod policy;
mod tool;

pub use agent::AgentError;
pub use classify::*;
pub use config::ConfigError;
pub use llm::LlmError;
pub use mail::MailError;
pub use policy::PolicyError;
pub use tool::ToolError;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),
    #[error("database error: {0}")]
    Database(sqlx::Error),
    #[error("database migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("mail gateway error: {0}")]
    Mail(#[from] MailError),
    #[error("llm provider error: {0}")]
    Llm(#[from] LlmError),
    #[error("tool execution error: {0}")]
    Tool(#[from] ToolError),
    #[error("agent loop error: {0}")]
    Agent(#[from] AgentError),
    #[error("security policy violation: {0}")]
    Policy(#[from] PolicyError),
    #[error("record not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("service unavailable: {0}")]
    Unavailable(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn internal(msg: impl std::fmt::Display) -> Self {
        AppError::Internal(msg.to_string())
    }

    pub fn invalid(msg: impl std::fmt::Display) -> Self {
        AppError::InvalidInput(msg.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    /// Central choke point for database failures: every `sqlx` error is
    /// surfaced as a `repository_error` observability event before wrapping.
    /// Callers that log a more specific event (`transaction_failed`,
    /// `database_connection_failed`) construct `AppError::Database` directly.
    fn from(e: sqlx::Error) -> Self {
        let error_type = crate::observability::errors::sqlx_error_type(&e);
        let text = e.to_string();
        crate::observability::system::repository_error("sql", error_type, &text);
        AppError::Database(e)
    }
}
