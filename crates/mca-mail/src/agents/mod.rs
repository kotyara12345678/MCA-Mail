//! AI Agents module — all specialized agent implementations.

mod classification;
mod communication;
mod handoff;
mod identity;
mod logistics;
mod prompt;
mod qualification;
mod research;
mod spam;

pub use classification::ClassificationAgent;
pub use communication::CommunicationAgent;
pub use handoff::HandoffAgent;
pub use identity::AgentIdentity;
pub use logistics::LogisticsExpertAgent;
pub use prompt::tool_use_prompt;
pub use qualification::LeadQualificationAgent;
pub use research::CompanyResearchAgent;
pub use spam::SpamAgent;

use async_trait::async_trait;
use serde_json::Value;

use crate::domain::AgentKind;
use crate::error::{AgentError, LlmError};

/// Common trait for all agents.
#[async_trait]
pub trait Agent: Send + Sync {
    fn kind(&self) -> AgentKind;

    /// Call the LLM and parse a structured response.
    ///
    /// Emits `llm_request_started` / `llm_request_completed` with token usage;
    /// prompts and raw model output are never logged.
    async fn call_llm_structured<T: serde::de::DeserializeOwned>(
        &self,
        context: &super::orchestration::AgentContext,
        system_prompt: &str,
        user_message: &str,
    ) -> Result<T, AgentError> {
        use std::time::Instant;

        let corr = crate::observability::Correlation::new(context.email_id, context.run_id);
        let tier = self.kind().tier();
        let agent = self.kind().as_str();
        let provider = context.llm.provider_name();
        let model = context.llm.resolve_model(tier);
        crate::observability::llm::llm_request_started(&corr, agent, provider, &model);
        let started = Instant::now();

        let response = context
            .llm
            .chat_for_tier(
                tier,
                system_prompt,
                vec![crate::llm::LlmMessage {
                    role: crate::llm::LlmRole::User,
                    content: user_message.to_string(),
                }],
                4096,
            )
            .await
            .map_err(|e| AgentError::InvalidOutput {
                agent: self.kind(),
                reason: e.to_string(),
            })?;

        crate::observability::llm::llm_request_completed(
            &corr,
            agent,
            provider,
            &model,
            started.elapsed().as_millis() as u64,
            response.prompt_tokens,
            response.completion_tokens,
        );

        // Try to parse as JSON, tolerating markdown fences and surrounding
        // prose from models that do not strictly honour the response contract.
        let content = response.content.trim();
        let json_str = prompt::extract_json_object(content).unwrap_or(content);

        serde_json::from_str::<T>(json_str).map_err(|e| AgentError::InvalidOutput {
            agent: self.kind(),
            reason: format!("JSON parse error: {e}"),
        })
    }

    fn agent_identity(&self) -> AgentIdentity {
        identity::identity_of(self.kind())
    }
}
