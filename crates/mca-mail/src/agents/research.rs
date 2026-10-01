//! Company Research Agent: checks company details from public sources.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Agent, AgentIdentity};
use crate::domain::{AgentKind, LeadId};
use crate::error::AgentError;
use crate::orchestration::AgentContext;

#[derive(Debug, Serialize, Deserialize)]
pub struct ResearchOutput {
    pub status: String,
    pub inn_found: Option<String>,
    pub company_name: Option<String>,
    pub legal_status: Option<String>,
    pub registration_date: Option<String>,
    pub region: Option<String>,
    pub notes: String,
    pub flags: Vec<String>,
}

pub struct CompanyResearchAgent {
    llm: std::sync::Arc<dyn crate::llm::LlmProvider>,
}

impl CompanyResearchAgent {
    pub fn new(llm: std::sync::Arc<dyn crate::llm::LlmProvider>) -> Self {
        CompanyResearchAgent { llm }
    }

    pub async fn research(
        &self,
        context: &AgentContext,
        _lead_id: LeadId,
    ) -> Result<ResearchOutput, AgentError> {
        // If company research is not configured, return not_configured
        let configured = context.settings.company_research;
        if !configured {
            return Ok(ResearchOutput {
                status: "not_configured".into(),
                inn_found: None,
                company_name: None,
                legal_status: None,
                registration_date: None,
                region: None,
                notes: "External company research is not configured yet. Contact the system administrator to enable it.".into(),
                flags: vec![],
            });
        }

        let system = r#"You are a company research agent. Check the provided company details against available information.
Extract any company identifiers present in the email and prepare for verification.

Respond with JSON: {"status": "pending|no_identifier", "inn_found": null, "company_name": null, "legal_status": null, "registration_date": null, "region": null, "notes": "...", "flags": []}"#;

        let user = format!(
            "Extract any company identifiers from:\n\nFrom: {}\nSubject: {}\nBody:\n{}",
            context.email.from_address,
            context.email.subject,
            truncate(&context.email.text_body, 2000)
        );

        let result: ResearchOutput = self.call_llm_structured(context, system, &user).await?;
        Ok(result)
    }
}

#[async_trait]
impl Agent for CompanyResearchAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::CompanyResearch
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}... [truncated {} chars]", &s[..max], s.len() - max)
    }
}
