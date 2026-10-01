//! Tool registry and tool execution failures.

use thiserror::Error;

use crate::domain::AgentKind;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool: {0}")]
    Unknown(String),
    #[error("agent {agent} is not permitted to call {tool}")]
    Denied { agent: AgentKind, tool: String },
    #[error("invalid arguments for {tool}: {reason}")]
    InvalidArgs { tool: String, reason: String },
    #[error("tool {tool} failed: {reason}")]
    Failed { tool: String, reason: String },
    #[error("tool {tool} timed out after {ms}ms")]
    Timeout { tool: String, ms: u64 },
    #[error("tool call limit reached for {0}")]
    LimitReached(String),
    #[error("result of {tool} failed validation: {reason}")]
    InvalidResult { tool: String, reason: String },
    #[error("tool {tool} is read-only and may not be called in this context")]
    DestructiveOperation { tool: String },
}
