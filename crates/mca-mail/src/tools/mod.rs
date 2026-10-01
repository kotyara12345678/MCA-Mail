//! Tool Registry: typed tools that agents can call.
//!
//! Agents interact with the system exclusively through these tools. Each tool
//! has defined arguments, permissions, and result validation.

pub mod crm;
pub mod mail;
pub mod support;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::AgentKind;
use crate::error::ToolError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub allowed_agents: Vec<AgentKind>,
    pub arg_schema: Value,
    pub destructive: bool,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub summary: String,
    pub data: Value,
    pub error: Option<String>,
}

type ToolHandler = Arc<dyn Fn(Value) -> Result<ToolResult, ToolError> + Send + Sync>;

#[derive(Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolEntry>,
}

struct ToolEntry {
    def: ToolDef,
    handler: ToolHandler,
}

impl Clone for ToolEntry {
    fn clone(&self) -> Self {
        ToolEntry {
            def: self.def.clone(),
            handler: self.handler.clone(),
        }
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        ToolRegistry {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, def: ToolDef, handler: ToolHandler) {
        self.tools
            .insert(def.name.clone(), ToolEntry { def, handler });
    }

    pub fn list_for_agent(&self, agent: AgentKind) -> Vec<&ToolDef> {
        self.tools
            .values()
            .filter(|e| e.def.allowed_agents.contains(&agent))
            .map(|e| &e.def)
            .collect()
    }

    pub fn describe_for_agent(&self, agent: AgentKind) -> Vec<ToolDef> {
        self.tools
            .values()
            .filter(|e| e.def.allowed_agents.contains(&agent))
            .map(|e| e.def.clone())
            .collect()
    }

    pub fn execute(
        &self,
        agent: AgentKind,
        tool_name: &str,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let entry = self
            .tools
            .get(tool_name)
            .ok_or_else(|| ToolError::Unknown(tool_name.to_string()))?;

        if !entry.def.allowed_agents.contains(&agent) {
            return Err(ToolError::Denied {
                agent,
                tool: tool_name.to_string(),
            });
        }

        if !args.is_object() {
            return Err(ToolError::InvalidArgs {
                tool: tool_name.to_string(),
                reason: "arguments must be a JSON object".into(),
            });
        }

        (entry.handler)(args)
    }

    pub fn get(&self, name: &str) -> Option<&ToolDef> {
        self.tools.get(name).map(|e| &e.def)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn execute_with_timeout(
    registry: &ToolRegistry,
    agent: AgentKind,
    tool_name: &str,
    args: Value,
    timeout: Duration,
) -> Result<ToolResult, ToolError> {
    let registry = registry.clone();
    let name = tool_name.to_string();
    let name_for_error = name.clone();

    tokio::time::timeout(timeout, async move { registry.execute(agent, &name, args) })
        .await
        .map_err(|_| ToolError::Timeout {
            tool: name_for_error,
            ms: timeout.as_millis() as u64,
        })?
}
