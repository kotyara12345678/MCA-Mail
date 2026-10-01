use serde::{Deserialize, Serialize};

use crate::domain::AgentKind;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolSettings {
    /// Wall-clock limit for a single tool invocation.
    pub call_timeout_seconds: u64,
    /// Per-task ceiling across all tools.
    pub max_calls_per_task: u32,
    /// Argument byte cap, so a model cannot smuggle a huge payload.
    pub max_argument_bytes: usize,
    /// Result byte cap before the value is truncated for the model.
    pub max_result_bytes: usize,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            call_timeout_seconds: 20,
            max_calls_per_task: 20,
            max_argument_bytes: 16 * 1024,
            max_result_bytes: 32 * 1024,
        }
    }
}

/// Per-agent enable switches and iteration budgets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub spam: bool,
    pub classification: bool,
    pub qualification: bool,
    pub logistics_expert: bool,
    pub company_research: bool,
    pub communication: bool,
    pub handoff: bool,
    pub max_iterations: u32,
    pub max_tool_calls: u32,
    pub task_timeout_seconds: u64,
    pub max_retries: u32,
    pub tools: ToolSettings,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            spam: true,
            classification: true,
            qualification: true,
            logistics_expert: true,
            company_research: true,
            communication: true,
            handoff: true,
            max_iterations: 8,
            max_tool_calls: 20,
            task_timeout_seconds: 180,
            max_retries: 3,
            tools: ToolSettings::default(),
        }
    }
}

impl AgentSettings {
    pub fn enabled(&self, agent: AgentKind) -> bool {
        match agent {
            AgentKind::Spam => self.spam,
            AgentKind::Classification => self.classification,
            AgentKind::LeadQualification => self.qualification,
            AgentKind::LogisticsExpert => self.logistics_expert,
            AgentKind::CompanyResearch => self.company_research,
            AgentKind::EmailCommunication => self.communication,
            AgentKind::Handoff => self.handoff,
        }
    }

    pub fn task_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.task_timeout_seconds.max(1))
    }

    pub fn tool_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.tools.call_timeout_seconds.max(1))
    }
}

/// Company pre-check wiring. Stays disabled until a source is approved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ResearchSettings {
    pub enabled: bool,
    /// Base URL of the approved registry integration.
    pub api_url: String,
    pub api_key: super::Secret,
    pub timeout_seconds: u64,
    /// Cache positive results for this long, in seconds.
    pub cache_ttl_seconds: u64,
}

impl ResearchSettings {
    /// A configured provider is only reported as ready when both the flag and
    /// the endpoint are present.
    pub fn provider_configured(&self) -> bool {
        self.enabled && !self.api_url.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RetentionSettings {
    /// Email bodies older than this are deleted or anonymized.
    pub email_days: i32,
    pub attachment_days: i32,
    pub event_days: i32,
    pub audit_days: i32,
    pub conversation_days: i32,
    /// When true, bodies are replaced with a placeholder instead of deleted, so
    /// the lead history stays coherent.
    pub anonymize_instead_of_delete: bool,
    /// Run the retention job at all.
    pub enabled: bool,
    pub check_interval_hours: u32,
}

impl Default for RetentionSettings {
    fn default() -> Self {
        Self {
            email_days: 365,
            attachment_days: 365,
            event_days: 180,
            audit_days: 1095,
            conversation_days: 730,
            anonymize_instead_of_delete: true,
            enabled: true,
            check_interval_hours: 24,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn research_is_not_configured_by_default() {
        let s = ResearchSettings::default();
        assert!(!s.provider_configured());
        let s = ResearchSettings {
            enabled: true,
            api_url: "https://registry.example/api".into(),
            ..Default::default()
        };
        assert!(s.provider_configured());
    }

    #[test]
    fn agent_switches_are_independent() {
        let mut s = AgentSettings::default();
        assert!(s.enabled(AgentKind::Spam));
        s.spam = false;
        assert!(!s.enabled(AgentKind::Spam));
        assert!(s.enabled(AgentKind::Classification));
    }
}
