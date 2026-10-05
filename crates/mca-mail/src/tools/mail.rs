//! Mail tools for agents: read, send, move, label, search.

use super::ToolDef;
use crate::domain::AgentKind;

pub fn define_list_emails() -> ToolDef {
    ToolDef {
        name: "list_emails".into(),
        description: "List recent inbound emails (subject, from, date)".into(),
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::CompanyResearch,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "minimum": 1, "maximum": 50 },
                "status": { "type": "string" }
            }
        }),
        destructive: false,
        timeout_seconds: 10,
    }
}

pub fn define_read_email() -> ToolDef {
    ToolDef {
        name: "read_email".into(),
        description: "Read the full content of a specific email by ID".into(),
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
                "email_id": { "type": "string" }
            },
            "required": ["email_id"]
        }),
        destructive: false,
        timeout_seconds: 10,
    }
}

pub fn define_read_thread() -> ToolDef {
    ToolDef {
        name: "read_thread".into(),
        description: "Read all messages in a conversation thread".into(),
        allowed_agents: vec![
            AgentKind::LeadQualification,
            AgentKind::LogisticsExpert,
            AgentKind::EmailCommunication,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "thread_id": { "type": "string" }
            },
            "required": ["thread_id"]
        }),
        destructive: false,
        timeout_seconds: 10,
    }
}

pub fn define_search_emails() -> ToolDef {
    ToolDef {
        name: "search_emails".into(),
        description: "Search emails by sender, subject, or date range".into(),
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::LeadQualification,
            AgentKind::CompanyResearch,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "from": { "type": "string" },
                "subject": { "type": "string" },
                "since_days": { "type": "integer" }
            }
        }),
        destructive: false,
        timeout_seconds: 10,
    }
}

pub fn define_get_email_attachments() -> ToolDef {
    ToolDef {
        name: "get_email_attachments".into(),
        description: "Get metadata of attachments for an email".into(),
        allowed_agents: vec![
            AgentKind::LeadQualification,
            AgentKind::CompanyResearch,
            AgentKind::LogisticsExpert,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "email_id": { "type": "string" }
            },
            "required": ["email_id"]
        }),
        destructive: false,
        timeout_seconds: 5,
    }
}

pub fn define_move_email() -> ToolDef {
    ToolDef {
        name: "move_email".into(),
        description: "Move email to a server-discovered folder role".into(),
        allowed_agents: vec![AgentKind::Spam, AgentKind::Classification],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "email_id": { "type": "string" },
                "role": { "type": "string", "enum": ["inbox", "archive", "drafts", "sent", "spam", "trash"] }
            },
            "required": ["email_id", "role"]
        }),
        destructive: true,
        timeout_seconds: 10,
    }
}

pub fn define_label_email() -> ToolDef {
    ToolDef {
        name: "label_email".into(),
        description: "Assign a label to an email".into(),
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::LeadQualification,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "email_id": { "type": "string" },
                "label": { "type": "string" }
            },
            "required": ["email_id", "label"]
        }),
        destructive: true,
        timeout_seconds: 5,
    }
}

pub fn define_archive_email() -> ToolDef {
    ToolDef {
        name: "archive_email".into(),
        description: "Archive an email (moves to archive folder)".into(),
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "email_id": { "type": "string" }
            },
            "required": ["email_id"]
        }),
        destructive: true,
        timeout_seconds: 10,
    }
}

pub fn define_mark_as_read() -> ToolDef {
    ToolDef {
        name: "mark_as_read".into(),
        description: "Mark email as read".into(),
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::LeadQualification,
            AgentKind::Handoff,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "email_id": { "type": "string" }
            },
            "required": ["email_id"]
        }),
        destructive: true,
        timeout_seconds: 5,
    }
}

pub fn define_create_email_draft() -> ToolDef {
    ToolDef {
        name: "create_email_draft".into(),
        description: "Create a draft email reply".into(),
        allowed_agents: vec![AgentKind::EmailCommunication],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "to": { "type": "string" },
                "subject": { "type": "string" },
                "body": { "type": "string" },
                "in_reply_to": { "type": "string" }
            },
            "required": ["to", "subject", "body"]
        }),
        destructive: true,
        timeout_seconds: 10,
    }
}

pub fn define_send_email() -> ToolDef {
    ToolDef {
        name: "send_email".into(),
        description: "Send an email (only when policy allows)".into(),
        allowed_agents: vec![AgentKind::EmailCommunication, AgentKind::Handoff],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "to": { "type": "string" },
                "subject": { "type": "string" },
                "body": { "type": "string" },
                "in_reply_to": { "type": "string" }
            },
            "required": ["to", "subject", "body"]
        }),
        destructive: true,
        timeout_seconds: 15,
    }
}

pub fn define_get_email_status() -> ToolDef {
    ToolDef {
        name: "get_email_status".into(),
        description: "Get current processing status of an email".into(),
        allowed_agents: vec![
            AgentKind::Spam,
            AgentKind::Classification,
            AgentKind::LeadQualification,
        ],
        arg_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "email_id": { "type": "string" }
            },
            "required": ["email_id"]
        }),
        destructive: false,
        timeout_seconds: 5,
    }
}

/// Build all mail tools into a vector for registration.
pub fn all_tools() -> Vec<ToolDef> {
    vec![
        define_list_emails(),
        define_read_email(),
        define_read_thread(),
        define_search_emails(),
        define_get_email_attachments(),
        define_move_email(),
        define_label_email(),
        define_archive_email(),
        define_mark_as_read(),
        define_create_email_draft(),
        define_send_email(),
        define_get_email_status(),
    ]
}

#[cfg(test)]
#[path = "mail_role_test.rs"]
mod tests;
