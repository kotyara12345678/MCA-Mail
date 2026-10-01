//! LLM request events: started, completed, retry, failed.

use tracing::{error, info, warn};

use super::{errors::clip, Correlation, EVENT_TARGET};

/// A model call was issued for an agent step (never logs the prompt).
pub fn llm_request_started(c: &Correlation, agent: &str, provider: &str, model: &str) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        agent,
        provider,
        model,
        "llm_request_started"
    );
}

/// A model call succeeded with token usage.
#[allow(clippy::too_many_arguments)]
pub fn llm_request_completed(
    c: &Correlation,
    agent: &str,
    provider: &str,
    model: &str,
    duration_ms: u64,
    input_tokens: i64,
    output_tokens: i64,
) {
    let total_tokens = input_tokens + output_tokens;
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        agent,
        provider,
        model,
        duration_ms,
        input_tokens,
        output_tokens,
        total_tokens,
        "llm_request_completed"
    );
}

/// The transport will retry after a retryable failure.
pub fn llm_retry(provider: &str, model: &str, retry_count: u32, error: &str) {
    warn!(
        target: EVENT_TARGET,
        provider,
        model,
        retry_count,
        error = clip(error, 200),
        "llm_retry"
    );
}

/// The model call failed after all attempts.
pub fn llm_failed(provider: &str, model: &str, error_type: &str, retry_count: u32, error: &str) {
    error!(
        target: EVENT_TARGET,
        provider,
        model,
        error_type,
        retry_count,
        error = clip(error, 300),
        "llm_failed"
    );
}
