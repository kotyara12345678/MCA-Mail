//! LLM transport and response-shape failures.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("llm request timed out after {0}ms")]
    Timeout(u64),
    #[error("llm provider returned HTTP {status}: {detail}")]
    Http { status: u16, detail: String },
    #[error("llm response is not valid JSON: {0}")]
    Malformed(String),
    #[error("llm response failed schema validation: {field}: {reason}")]
    Schema { field: String, reason: String },
    #[error("llm budget exhausted: {0}")]
    BudgetExceeded(String),
    #[error("llm unavailable: {0}")]
    Unavailable(String),
    #[error("llm request rejected by circuit breaker: {0}")]
    CircuitOpen(String),
    #[error("llm internal error: {0}")]
    Internal(String),
}

impl LlmError {
    /// Whether the caller should retry with backoff.
    ///
    /// A malformed or schema-invalid response is deliberately *not* retryable:
    /// repeating the same request against the same broken model wastes budget
    /// and hides a configuration problem.
    pub fn is_retryable(&self) -> bool {
        match self {
            LlmError::Timeout(_) | LlmError::Unavailable(_) => true,
            LlmError::Http { status, .. } => *status == 429 || *status >= 500,
            LlmError::CircuitOpen(_) => true,
            _ => false,
        }
    }
}
