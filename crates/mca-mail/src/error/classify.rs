//! Classification helpers for [`AppError`]: codes, HTTP status, retry policy.

use super::{AppError, MailError};

impl AppError {
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
