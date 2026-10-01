//! Tool execution events: started, completed, failed, timeout.

use tracing::{error, info};

use super::{errors::clip, EVENT_TARGET};

/// A tool began executing for an agent.
pub fn tool_started(agent: &str, tool: &str) {
    info!(target: EVENT_TARGET, agent, tool, "tool_started");
}

/// A tool finished successfully; `result` is its short summary.
pub fn tool_completed(agent: &str, tool: &str, result: &str, duration_ms: u64) {
    info!(
        target: EVENT_TARGET,
        agent,
        tool,
        result = clip(result, 160),
        duration_ms,
        "tool_completed"
    );
}

/// A tool returned an error.
pub fn tool_failed(agent: &str, tool: &str, error_type: &str, error: &str, duration_ms: u64) {
    error!(
        target: EVENT_TARGET,
        agent,
        tool,
        error_type,
        error = clip(error, 300),
        duration_ms,
        "tool_failed"
    );
}

/// A tool exceeded its timeout budget and was aborted.
pub fn tool_timeout(agent: &str, tool: &str, duration_ms: u64) {
    error!(
        target: EVENT_TARGET,
        agent,
        tool,
        duration_ms,
        "tool_timeout"
    );
}
