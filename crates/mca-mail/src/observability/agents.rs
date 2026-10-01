//! Agent execution events: started, completed, failed.

use tracing::{error, info};

use super::{Correlation, EVENT_TARGET};

/// An agent step began for the current email run.
pub fn agent_started(c: &Correlation, agent: &str) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        agent,
        "agent_started"
    );
}

/// An agent step returned a value; `result` is its verdict/outcome.
pub fn agent_completed(c: &Correlation, agent: &str, result: &str, duration_ms: u64) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        agent,
        result,
        duration_ms,
        "agent_completed"
    );
}

/// An agent step failed (bad model output, tool or provider error).
pub fn agent_failed(c: &Correlation, agent: &str, error_type: &str, error: &str, duration_ms: u64) {
    error!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        agent,
        error_type,
        error = super::errors::clip(error, 300),
        duration_ms,
        "agent_failed"
    );
}
