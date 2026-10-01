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
    #[error("agent {agent} is disabled by configuration")]
    Disabled { agent: AgentKind },
}
