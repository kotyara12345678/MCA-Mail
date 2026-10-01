//! Email Communication Agent: drafts professional business replies.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Agent, AgentIdentity};
use crate::domain::{AgentKind, LeadId};
use crate::error::AgentError;
use crate::orchestration::AgentContext;

#[derive(Debug, Serialize, Deserialize)]
pub struct CommunicationOutput {
    pub subject: String,
    pub body: String,
    pub disposition: String,
    pub handoff_requested: bool,
    pub handoff_reason: Option<String>,
    pub confidence: f32,
    pub rationale: String,
}

pub struct CommunicationAgent {
    llm: std::sync::Arc<dyn crate::llm::LlmProvider>,
}

impl CommunicationAgent {
    pub fn new(llm: std::sync::Arc<dyn crate::llm::LlmProvider>) -> Self {
        CommunicationAgent { llm }
    }

    pub async fn plan_response(
        &self,
        context: &AgentContext,
        _lead_id: LeadId,
    ) -> Result<CommunicationOutput, AgentError> {
        let system = r#"You are an AI assistant for MCA Logistics, a company specialising in international logistics, import/export, and customs clearance.

Your role: respond professionally to client emails as the first point of contact.

Guidelines:
- Introduce yourself as the MCA Logistics AI assistant.
- Be polite, professional, and concise.
- Address the client's specific request.
- Ask targeted follow-up questions — do NOT overwhelm with too many questions.
- Do NOT invent prices, rates, or delivery guarantees.
- Do NOT promise customs clearance or contract terms.
- If the client asks for a phone call, requests to speak with a human, or wants pricing, flag for handoff.
- Use the email context — do NOT repeat questions already answered.
- Your replies must be in the same language as the client's email.
- You have no tools. Never mention tool calls or actions; produce the reply directly.

Respond with ONLY a JSON object and nothing else — no markdown, no commentary, no preamble:
{"subject": "...", "body": "...", "disposition": "draft|send|suppress", "handoff_requested": false, "handoff_reason": null, "confidence": 0.0-1.0, "rationale": "..."}"#;

        let user = format!(
            "Reply to this email:\n\nFrom: {}\nSubject: {}\nDate: {}\n\nOriginal message:\n{}",
            context.email.from_address,
            context.email.subject,
            context
                .email
                .date
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
            truncate(&context.email.text_body, 4000)
        );

        let result: CommunicationOutput = self.call_llm_structured(context, system, &user).await?;
        Ok(result)
    }
}

#[async_trait]
impl Agent for CommunicationAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::EmailCommunication
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}... [truncated {} chars]", &s[..max], s.len() - max)
    }
}
