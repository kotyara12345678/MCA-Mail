//! Logistics Expert Agent: provides professional logistics advice.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Agent, AgentIdentity};
use crate::domain::{AgentKind, LeadId};
use crate::error::AgentError;
use crate::orchestration::AgentContext;

#[derive(Debug, Serialize, Deserialize)]
pub struct LogisticsOutput {
    pub explanation: String,
    pub questions: Vec<String>,
    pub escalate_topics: Vec<String>,
    pub forbidden_claims_avoided: Vec<String>,
    pub confidence: f32,
}

pub struct LogisticsExpertAgent {
    llm: std::sync::Arc<dyn crate::llm::LlmProvider>,
}

impl LogisticsExpertAgent {
    pub fn new(llm: std::sync::Arc<dyn crate::llm::LlmProvider>) -> Self {
        LogisticsExpertAgent { llm }
    }

    pub async fn consult(
        &self,
        context: &AgentContext,
        _lead_id: LeadId,
    ) -> Result<LogisticsOutput, AgentError> {
        let system = r#"You are a senior logistics and international trade expert for MCA Logistics.
Your expertise covers:
- International freight (road, rail, sea, air, multimodal)
- Customs clearance and documentation
- Import/export regulations
- International trade and procurement
- Cargo insurance

Rules:
- Do NOT provide specific pricing or rates.
- Do NOT guarantee delivery times.
- Do NOT promise customs clearance.
- Do NOT advise on sanctions evasion or regulatory loopholes.
- If the query involves regulated goods (sanctions, dual-use, hazardous), flag for escalation.
- Be helpful and professional, but know when to hand off to a human.
- Explain services MCA can provide without overcommitting.

Respond with JSON: {"explanation": "...", "questions": ["..."], "escalate_topics": ["..."], "forbidden_claims_avoided": ["..."], "confidence": 0.0-1.0}"#;

        let user = format!(
            "Customer enquiry from: {}\n\nContext:\n{}",
            context.email.from_address,
            truncate(&context.email.text_body, 3000)
        );

        let result: LogisticsOutput = self.call_llm_structured(context, system, &user).await?;
        Ok(result)
    }
}

#[async_trait]
impl Agent for LogisticsExpertAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::LogisticsExpert
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}... [truncated {} chars]", &s[..max], s.len() - max)
    }
}
