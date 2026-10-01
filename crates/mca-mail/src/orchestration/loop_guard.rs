//! Loop guards: iteration limits, tool call limits, budget tracking.

use crate::domain::AgentKind;
use crate::error::AgentError;

/// Tracks iterations and enforces the per-agent maximum.
pub struct IterationGuard {
    max: u32,
    count: u32,
}

impl IterationGuard {
    pub fn new(max: u32) -> Self {
        IterationGuard { max, count: 0 }
    }

    pub fn advance(&mut self) -> Result<(), AgentError> {
        self.count += 1;
        if self.count > self.max {
            return Err(AgentError::IterationLimit {
                agent: AgentKind::Spam, // placeholder — caller should set real agent
                limit: self.max,
            });
        }
        Ok(())
    }

    pub fn remaining(&self) -> u32 {
        self.max.saturating_sub(self.count)
    }
}

/// Tracks tool calls and enforces the per-task maximum.
pub struct ToolCallGuard {
    max: u32,
    count: u32,
}

impl ToolCallGuard {
    pub fn new(max: u32) -> Self {
        ToolCallGuard { max, count: 0 }
    }

    pub fn record_call(&mut self) -> Result<(), AgentError> {
        self.count += 1;
        if self.count > self.max {
            return Err(AgentError::ToolCallLimit {
                agent: AgentKind::LeadQualification,
                limit: self.max,
            });
        }
        Ok(())
    }

    pub fn remaining(&self) -> u32 {
        self.max.saturating_sub(self.count)
    }
}

/// Tracks token and cost budgets.
pub struct BudgetTracker {
    prompt_tokens: i64,
    completion_tokens: i64,
}

impl BudgetTracker {
    pub fn new() -> Self {
        BudgetTracker {
            prompt_tokens: 0,
            completion_tokens: 0,
        }
    }

    pub fn add_tokens(&mut self, prompt: i64, completion: i64) {
        self.prompt_tokens += prompt;
        self.completion_tokens += completion;
    }

    pub fn total_tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }

    pub fn prompt_tokens(&self) -> i64 {
        self.prompt_tokens
    }

    pub fn completion_tokens(&self) -> i64 {
        self.completion_tokens
    }
}

impl Default for BudgetTracker {
    fn default() -> Self {
        Self::new()
    }
}
