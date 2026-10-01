//! CRM tools: lead management, creation, search, handoff.

use super::ToolDef;
use crate::domain::AgentKind;

pub fn define_create_lead() -> ToolDef {
    ToolDef {
        name: "create_lead".into(),
        description: "Create a new lead from an inbound email conversation".into(),
        allowed_agents: vec![
            AgentKind::LeadQualification,
            AgentKind::EmailCommunication,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "contact_email": { "type": "string" },
                "company_name": { "type": "string" },
                "company_inn": { "type": "string" },
                "summary": { "type": "string" },
                "scope": { "type": "string", "enum": ["transport", "customs", "procurement", "full_import"] }
            },
            "required": ["contact_email"]
        }),
        destructive: false,
        timeout_seconds: 10,
    }
}

pub fn define_update_lead() -> ToolDef {
    ToolDef {
        name: "update_lead".into(),
        description: "Update lead fields like company name, INN or summary".into(),
        allowed_agents: vec![AgentKind::LeadQualification, AgentKind::EmailCommunication],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "lead_id": { "type": "string" },
                "company_name": { "type": "string" },
                "company_inn": { "type": "string" },
                "summary": { "type": "string" },
                "status": { "type": "string", "enum": ["new","processing","awaiting_customer","qualified","needs_human_review","handed_off","in_progress","won","lost","closed"] }
            },
            "required": ["lead_id"]
        }),
        destructive: false,
        timeout_seconds: 10,
    }
}

pub fn define_get_lead() -> ToolDef {
    ToolDef {
        name: "get_lead".into(),
        description: "Retrieve lead details by ID".into(),
        allowed_agents: vec![
            AgentKind::LeadQualification,
            AgentKind::LogisticsExpert,
            AgentKind::EmailCommunication,
            AgentKind::CompanyResearch,
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

pub fn define_search_leads() -> ToolDef {
    ToolDef {
        name: "search_leads".into(),
        description: "Search leads by email, company name or INN".into(),
        allowed_agents: vec![
            AgentKind::LeadQualification,
            AgentKind::CompanyResearch,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "email": { "type": "string" },
                "company_name": { "type": "string" },
                "inn": { "type": "string" }
            }
        }),
        destructive: false,
        timeout_seconds: 5,
    }
}

pub fn define_add_note() -> ToolDef {
    ToolDef {
        name: "add_lead_note".into(),
        description: "Append a note to a lead (visible to managers)".into(),
        allowed_agents: vec![
            AgentKind::LeadQualification,
            AgentKind::LogisticsExpert,
            AgentKind::CompanyResearch,
            AgentKind::EmailCommunication,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "lead_id": { "type": "string" },
                "note": { "type": "string" }
            },
            "required": ["lead_id", "note"]
        }),
        destructive: false,
        timeout_seconds: 5,
    }
}

pub fn define_update_lead_status() -> ToolDef {
    ToolDef {
        name: "update_lead_status".into(),
        description: "Change lead status".into(),
        allowed_agents: vec![
            AgentKind::LeadQualification,
            AgentKind::EmailCommunication,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "lead_id": { "type": "string" },
                "status": { "type": "string", "enum": ["new","processing","awaiting_customer","qualified","needs_human_review","handed_off","in_progress","won","lost","closed"] }
            },
            "required": ["lead_id", "status"]
        }),
        destructive: false,
        timeout_seconds: 5,
    }
}

pub fn define_handoff_to_manager() -> ToolDef {
    ToolDef {
        name: "handoff_to_manager".into(),
        description: "Transfer a qualified lead to a human manager".into(),
        allowed_agents: vec![AgentKind::Handoff],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "lead_id": { "type": "string" },
                "reason": { "type": "string" },
                "priority": { "type": "integer", "minimum": 1, "maximum": 100 }
            },
            "required": ["lead_id", "reason"]
        }),
        destructive: false,
        timeout_seconds: 10,
    }
}

pub fn define_get_manager_contacts() -> ToolDef {
    ToolDef {
        name: "get_manager_contacts".into(),
        description: "Get list of available managers for handoff".into(),
        allowed_agents: vec![AgentKind::Handoff],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        destructive: false,
        timeout_seconds: 5,
    }
}

pub fn define_get_lead_conversation() -> ToolDef {
    ToolDef {
        name: "get_lead_conversation".into(),
        description: "Retrieve full conversation history for a lead".into(),
        allowed_agents: vec![
            AgentKind::LogisticsExpert,
            AgentKind::EmailCommunication,
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

/// Build all CRM tools into a vector for registration.
pub fn all_tools() -> Vec<ToolDef> {
    vec![
        define_create_lead(),
        define_update_lead(),
        define_get_lead(),
        define_search_leads(),
        define_add_note(),
        define_update_lead_status(),
        define_handoff_to_manager(),
        define_get_manager_contacts(),
        define_get_lead_conversation(),
    ]
}
