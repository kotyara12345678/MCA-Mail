//! Top-level error type.
//!
//! Every fallible boundary returns [`AppError`] so the API layer can map
//! failures onto stable HTTP responses without string matching.

mod agent;
mod config;
mod llm;
mod mail;
mod policy;
mod tool;

pub use agent::AgentError;
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
    Database(#[from] sqlx::Error),
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

    /// Stable machine-readable code for the unified API error envelope.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Config(_) => "config_error",
            AppError::Database(_) | AppError::Migration(_) => "database_error",
            AppError::Mail(_) => "mail_error",
            AppError::Llm(_) => "llm_error",
            AppError::Tool(_) => "tool_error",
            AppError::Agent(_) => "agent_error",
            AppError::Policy(_) => "policy_violation",
            AppError::NotFound(_) => "not_found",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::Unauthorized(_) => "unauthorized",
            AppError::Forbidden(_) => "forbidden",
            AppError::Conflict(_) => "conflict",
            AppError::Unavailable(_) => "service_unavailable",
            AppError::Internal(_) => "internal_error",
        }
    }

    /// HTTP status for this failure. Kept beside `code` so the two never drift.
    pub fn http_status(&self) -> u16 {
        match self {
            AppError::NotFound(_) => 404,
            AppError::InvalidInput(_) | AppError::Config(_) => 400,
            AppError::Unauthorized(_) => 401,
            AppError::Forbidden(_) | AppError::Policy(_) => 403,
            AppError::Conflict(_) => 409,
            AppError::Unavailable(_) | AppError::Mail(_) | AppError::Llm(_) => 503,
            AppError::Tool(_) | AppError::Agent(_) => 422,
            _ => 500,
        }
    }

    /// Whether a worker should schedule a retry rather than mark the run dead.
    pub fn is_retryable(&self) -> bool {
        match self {
            AppError::Llm(e) => e.is_retryable(),
            AppError::Mail(m) => matches!(m, MailError::Connect(_) | MailError::Unavailable(_)),
            AppError::Unavailable(_) | AppError::Database(_) => true,
            _ => false,
        }
    }
}
