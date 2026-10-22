//! Agent loop control-flow failures.

use thiserror::Error;

use crate::domain::AgentKind;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent {agent} exceeded {limit} iterations")]
    IterationLimit { agent: AgentKind, limit: u32 },
    #[error("agent {agent} exceeded {limit} tool calls")]
    ToolCallLimit { agent: AgentKind, limit: u32 },
    #[error("agent {agent} task exceeded {seconds}s")]
    TaskTimeout { agent: AgentKind, seconds: u64 },
    #[error("agent {agent} exceeded its token/cost budget")]
    BudgetExceeded { agent: AgentKind },
    #[error("agent {agent} produced an invalid structured response: {reason}")]
    InvalidOutput { agent: AgentKind, reason: String },
    #[error("agent {agent} provider error: {source}")]
    Provider {
        agent: AgentKind,
        #[source]
        source: crate::error::LlmError,
    },
    #[error("agent {agent} is disabled by configuration")]
    Disabled { agent: AgentKind },
}

impl AgentError {
    /// A provider hiccup (429, 5xx, timeout) is not a verdict about the
    /// message: the queue should retry it instead of parking it for review.
    pub fn is_retryable(&self) -> bool {
        match self {
            AgentError::Provider { source, .. } => source.is_retryable(),
            _ => false,
        }
    }
}
