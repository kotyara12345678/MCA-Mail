//! Error classification helpers shared by the event helpers.

use crate::error::{AppError, LlmError};

/// Short stable error type for an [`AppError`] (never the free text).
pub fn app_error_type(e: &AppError) -> &'static str {
    match e {
        AppError::Config(_) => "config",
        AppError::Database(_) => "database",
        AppError::Migration(_) => "migration",
        AppError::Mail(_) => "mail",
        AppError::Llm(_) => "llm",
        AppError::Tool(_) => "tool",
        AppError::Agent(_) => "agent",
        AppError::Policy(_) => "policy",
        AppError::NotFound(_) => "not_found",
        AppError::InvalidInput(_) => "invalid_input",
        AppError::Unauthorized(_) => "unauthorized",
        AppError::Forbidden(_) => "forbidden",
        AppError::Conflict(_) => "conflict",
        AppError::Unavailable(_) => "unavailable",
        AppError::Internal(_) => "internal",
    }
}

/// Short stable error type for an [`LlmError`] transport failure.
pub fn llm_error_type(e: &LlmError) -> &'static str {
    match e {
        LlmError::Timeout(_) => "timeout",
        LlmError::Http { .. } => "http",
        LlmError::Malformed(_) => "malformed",
        LlmError::Schema { .. } => "schema",
        LlmError::BudgetExceeded(_) => "budget_exceeded",
        LlmError::Unavailable(_) => "unavailable",
        LlmError::CircuitOpen(_) => "circuit_open",
        LlmError::Internal(_) => "internal",
    }
}

/// Short stable error type for a `sqlx` failure.
pub fn sqlx_error_type(e: &sqlx::Error) -> &'static str {
    match e {
        sqlx::Error::Io(_) => "io",
        sqlx::Error::Database(_) => "database",
        sqlx::Error::PoolTimedOut => "pool_timed_out",
        sqlx::Error::PoolClosed => "pool_closed",
        sqlx::Error::WorkerCrashed => "worker_crashed",
        sqlx::Error::Migrate(_) => "migrate",
        sqlx::Error::Protocol(_) => "protocol",
        _ => "other",
    }
}

/// Short stable error type for a tool failure.
pub fn tool_error_type(e: &crate::error::ToolError) -> &'static str {
    use crate::error::ToolError;
    match e {
        ToolError::Unknown(_) => "unknown",
        ToolError::Denied { .. } => "denied",
        ToolError::InvalidArgs { .. } => "invalid_args",
        ToolError::Failed { .. } => "failed",
        ToolError::Timeout { .. } => "timeout",
        ToolError::LimitReached(_) => "limit_reached",
        ToolError::InvalidResult { .. } => "invalid_result",
        ToolError::DestructiveOperation { .. } => "destructive_operation",
    }
}

/// Bound free-form error text so a huge provider body never floods a log line.
pub fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}
