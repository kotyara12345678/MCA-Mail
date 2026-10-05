//! Tool Registry: typed tools that agents can call.
//!
//! Agents interact with the system exclusively through these tools. Each tool
//! has defined arguments, permissions, and result validation.

pub mod crm;
pub mod mail;
pub mod mode_filter;
pub mod support;

pub use mode_filter::{allowed_in, filter_for_mode, mutates_mailbox};

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

        // Mailbox action arguments, captured before the handler consumes them.
        let email_id = args
            .get("email_id")
            .and_then(Value::as_str)
            .and_then(|s| uuid::Uuid::parse_str(s).ok());
        let folder = args
            .get("folder")
            .and_then(Value::as_str)
            .map(str::to_string);
        let label = args
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string);

        let started = std::time::Instant::now();
        crate::observability::tools::tool_started(agent.as_str(), tool_name);
        let result = (entry.handler)(args);
        let duration_ms = started.elapsed().as_millis() as u64;

        match &result {
            Ok(r) => {
                crate::observability::tools::tool_completed(
                    agent.as_str(),
                    tool_name,
                    &r.summary,
                    duration_ms,
                );
                // The side effects below belong to the action, not to the
                // attempt: a handler that answered "nothing happened" must not
                // leave an audit entry saying an email was moved.
                if r.success {
                    match tool_name {
                        "move_email" => crate::observability::actions::email_moved(
                            email_id,
                            folder.as_deref().unwrap_or("unknown"),
                        ),
                        "archive_email" => {
                            crate::observability::actions::email_moved(email_id, "archive")
                        }
                        "label_email" => crate::observability::actions::email_labeled(
                            email_id,
                            label.as_deref().unwrap_or("unknown"),
                        ),
                        _ => {}
                    }
                }
            }
            Err(e) => crate::observability::tools::tool_failed(
                agent.as_str(),
                tool_name,
                crate::observability::errors::tool_error_type(e),
                &e.to_string(),
                duration_ms,
            ),
        }
        result
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
    let ms = timeout.as_millis() as u64;

    match tokio::time::timeout(timeout, async move { registry.execute(agent, &name, args) }).await {
        Ok(result) => result,
        Err(_) => {
            crate::observability::tools::tool_timeout(agent.as_str(), tool_name, ms);
            Err(ToolError::Timeout {
                tool: name_for_error,
                ms,
            })
        }
    }
}
