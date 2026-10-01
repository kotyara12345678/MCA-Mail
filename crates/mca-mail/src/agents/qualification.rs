//! Lead Qualification Agent: extracts commercial parameters from client emails.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Agent, AgentIdentity};
use crate::domain::{AgentKind, EmailId, LeadId};
use crate::error::AgentError;
use crate::orchestration::AgentContext;

#[derive(Debug, Serialize, Deserialize)]
pub struct QualificationOutput {
    pub lead_id: Option<LeadId>,
    pub first_email_id: Option<EmailId>,
    pub company_name: Option<String>,
    pub contact_name: Option<String>,
    pub contact_phone: Option<String>,
    pub summary: String,
    pub scope: String,
    pub needs_expert: bool,
    pub questions: Vec<String>,
    pub confidence: f32,
    pub regulated_topics: Vec<String>,
}

pub struct LeadQualificationAgent {
    llm: std::sync::Arc<dyn crate::llm::LlmProvider>,
}

impl LeadQualificationAgent {
    pub fn new(llm: std::sync::Arc<dyn crate::llm::LlmProvider>) -> Self {
        LeadQualificationAgent { llm }
    }

    pub async fn qualify(&self, context: &AgentContext) -> Result<QualificationOutput, AgentError> {
        let system = r#"You are a lead qualification agent for MCA Logistics, a company providing international logistics services.
Analyse the email and extract commercial details.

Extract what information is available:
- Company name, contact person, phone
- Type of request (transport, customs, procurement, full_import)
- Cargo details if mentioned (weight, volume, origin, destination)
- Whether the client needs expert advice

Rules:
- Do NOT invent missing data.
- Distinguish stated facts from your inferences.
- Return questions for missing critical information.
- If the email asks for a phone call, set needs_expert=true.

Respond with JSON: {"lead_id": null, "first_email_id": null, "company_name": "...", "contact_name": "...", "contact_phone": null, "summary": "...", "scope": "transport|customs|procurement|full_import", "needs_expert": false, "questions": ["..."], "confidence": 0.0-1.0, "regulated_topics": []}"#;

        let user = format!(
            "From: {}\nSubject: {}\n\nBody:\n{}",
            context.email.from_address,
            context.email.subject,
            truncate(&context.email.text_body, 4000)
        );

        let mut result: QualificationOutput =
            self.call_llm_structured(context, system, &user).await?;

        // Fill in lead_id and email_id from context
        result.first_email_id = Some(context.email_id);

        Ok(result)
    }
}

#[async_trait]
impl Agent for LeadQualificationAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::LeadQualification
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}... [truncated {} chars]", &s[..max], s.len() - max)
    }
}
