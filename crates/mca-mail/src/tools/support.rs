//! Support tools: research, review requests, processing policy, agent state.

use super::ToolDef;
use crate::domain::AgentKind;

pub fn define_lookup_company_by_inn() -> ToolDef {
    ToolDef {
        name: "lookup_company_by_inn".into(),
        description: "Look up company details by INN from approved registries".into(),
        allowed_agents: vec![AgentKind::CompanyResearch, AgentKind::LeadQualification],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "inn": { "type": "string", "minLength": 10, "maxLength": 12 }
            },
            "required": ["inn"]
        }),
        destructive: false,
        timeout_seconds: 30,
    }
}

pub fn define_get_company_public_data() -> ToolDef {
    ToolDef {
        name: "get_company_public_data".into(),
        description: "Get publicly available data for a company (name, status, region)".into(),
        allowed_agents: vec![AgentKind::CompanyResearch, AgentKind::LeadQualification],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "company_name": { "type": "string" },
                "inn": { "type": "string" }
            }
        }),
        destructive: false,
        timeout_seconds: 15,
    }
}

pub fn define_get_company_research_status() -> ToolDef {
    ToolDef {
        name: "get_company_research_status".into(),
        description: "Check whether company research has been performed on a lead".into(),
        allowed_agents: vec![
            AgentKind::CompanyResearch,
            AgentKind::LeadQualification,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "lead_id": { "type": "string" }
            },
            "required": ["lead_id"]
        }),
        destructive: false,
        timeout_seconds: 5,
    }
}

/// Orchestrator-only tools (not callable by LLM agents directly).
pub fn define_request_human_review() -> ToolDef {
    ToolDef {
        name: "request_human_review".into(),
        description: "Flag a case for manual review by a manager".into(),
        // Only the orchestrator can call this.
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::LeadQualification,
            AgentKind::LogisticsExpert,
            AgentKind::EmailCommunication,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "email_id": { "type": "string" },
                "reason": { "type": "string" },
                "details": { "type": "string" }
            },
            "required": ["email_id", "reason"]
        }),
        destructive: false,
        timeout_seconds: 5,
    }
}

pub fn define_get_processing_policy() -> ToolDef {
    ToolDef {
        name: "get_processing_policy".into(),
        description: "Retrieve current processing policy (mode, limits)".into(),
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::LeadQualification,
            AgentKind::LogisticsExpert,
            AgentKind::EmailCommunication,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        destructive: false,
        timeout_seconds: 3,
    }
}

pub fn define_get_agent_state() -> ToolDef {
    ToolDef {
        name: "get_agent_state".into(),
        description: "Get the current agent's state and conversation context".into(),
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::LeadQualification,
            AgentKind::LogisticsExpert,
            AgentKind::CompanyResearch,
            AgentKind::EmailCommunication,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        destructive: false,
        timeout_seconds: 3,
    }
}

pub fn define_record_processing_event() -> ToolDef {
    ToolDef {
        name: "record_processing_event".into(),
        description: "Record an internal processing event for audit".into(),
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::LeadQualification,
            AgentKind::LogisticsExpert,
            AgentKind::CompanyResearch,
            AgentKind::EmailCommunication,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "event_type": { "type": "string" },
                "details": { "type": "object" }
            },
            "required": ["event_type"]
        }),
        destructive: false,
        timeout_seconds: 5,
    }
}

/// Build all support tools.
pub fn all_tools() -> Vec<ToolDef> {
    vec![
        define_lookup_company_by_inn(),
        define_get_company_public_data(),
        define_get_company_research_status(),
        define_request_human_review(),
        define_get_processing_policy(),
        define_get_agent_state(),
        define_record_processing_event(),
    ]
}
