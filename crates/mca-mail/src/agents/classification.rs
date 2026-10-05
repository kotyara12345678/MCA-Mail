//! Classification Agent: categorises inbound emails.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Agent, AgentIdentity};
use crate::domain::{AgentKind, EmailCategory};
use crate::error::AgentError;
use crate::orchestration::AgentContext;

#[derive(Debug, Serialize, Deserialize)]
pub struct ClassificationOutput {
    pub category: EmailCategory,
    pub confidence: f32,
    #[serde(deserialize_with = "crate::domain::flex::string")]
    pub explanation: String,
    pub requires_human: bool,
    #[serde(deserialize_with = "crate::domain::flex::string")]
    pub suggested_action: String,
}

pub struct ClassificationAgent {
    llm: std::sync::Arc<dyn crate::llm::LlmProvider>,
}

impl ClassificationAgent {
    pub fn new(llm: std::sync::Arc<dyn crate::llm::LlmProvider>) -> Self {
        ClassificationAgent { llm }
    }

    pub async fn classify(
        &self,
        context: &AgentContext,
    ) -> Result<ClassificationOutput, AgentError> {
        let system = r#"You are a classification agent for a logistics company (MCA Logistics).
Analyse the inbound email and determine its category.

Categories:
- new_lead: First-time commercial enquiry about logistics/import/export services
- existing_client: Message from a known client
- partner: Message from a partner or supplier
- transport_request: Request for transportation only (no extra services)
- full_import_request: Request for full import services (buy, ship, clear customs)
- customs_request: Only customs clearance needed
- procurement_request: Client wants MCA to source/purchase goods abroad
- document_request: Request for invoices, certificates, other documents
- complaint: Customer complaint or dispute
- internal: Internal corporate communication
- advertisement: Third-party promotional content
- spam: Unsolicited bulk mail
- business_inquiry: Genuine commercial enquiry that does not fit another service category
- other: Fits no business shape at all (auto-replies, personal mail, nonsense)
- uncertain: Not enough information to classify

Respond with: {"category": "...", "confidence": 0.0-1.0, "explanation": "...", "requires_human": false, "suggested_action": "..."}"#;

        let user = format!(
            "From: {}\nSubject: {}\n\nBody:\n{}",
            context.email.from_address,
            context.email.subject,
            truncate(&context.email.text_body, 4000)
        );

        let result: ClassificationOutput = self.call_llm_structured(context, system, &user).await?;
        Ok(result)
    }
}

#[async_trait]
impl Agent for ClassificationAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Classification
    }
}

fn truncate(s: &str, max: usize) -> String {
    super::prompt::truncate_for_prompt(s, max)
}
