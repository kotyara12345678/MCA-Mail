//! Handoff Agent: prepares structured handoff packages for human managers.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Agent, AgentIdentity};
use crate::domain::{AgentKind, LeadId};
use crate::error::AgentError;
use crate::orchestration::AgentContext;

#[derive(Debug, Serialize, Deserialize)]
pub struct HandoffOutput {
    pub reason: String,
    pub priority: i32,
    pub cargo_summary: String,
    pub route_summary: String,
    pub requested_service: String,
    pub missing_information: Vec<String>,
    pub open_questions: Vec<String>,
    pub conversation_digest: String,
    pub checks_performed: Vec<String>,
    pub unresolved_topics: Vec<String>,
    pub original_request: String,
}

pub struct HandoffAgent {
    llm: std::sync::Arc<dyn crate::llm::LlmProvider>,
}

impl HandoffAgent {
    pub fn new(llm: std::sync::Arc<dyn crate::llm::LlmProvider>) -> Self {
        HandoffAgent { llm }
    }

    pub async fn handoff(
        &self,
        context: &AgentContext,
        _lead_id: LeadId,
    ) -> Result<HandoffOutput, AgentError> {
        let system = r#"You are the handoff agent for MCA Logistics.
Your task is to prepare a structured summary for a human manager when a lead is being transferred.

The handoff must include:
- Original request summary
- What information has been collected
- What is still missing
- Any open questions the client has NOT answered yet
- Any compliance or regulatory flags
- Reason for handoff (e.g., pricing requested, phone call requested, complex customs matter)
- Priority level (1-100, where 1 is highest)

Be thorough — the manager needs to understand the full context without reading the entire email thread.

Respond with JSON: {"reason": "...", "priority": 50, "cargo_summary": "...", "route_summary": "...", "requested_service": "...", "missing_information": ["..."], "open_questions": ["..."], "conversation_digest": "...", "checks_performed": ["..."], "unresolved_topics": ["..."], "original_request": "..."}"#;

        let user = format!(
            "Prepare handoff for:\n\nFrom: {}\nSubject: {}\n\nEmail content:\n{}",
            context.email.from_address,
            context.email.subject,
            truncate(&context.email.text_body, 4000)
        );

        let result: HandoffOutput = self.call_llm_structured(context, system, &user).await?;
        Ok(result)
    }
}

#[async_trait]
impl Agent for HandoffAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Handoff
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}... [truncated {} chars]", &s[..max], s.len() - max)
    }
}
