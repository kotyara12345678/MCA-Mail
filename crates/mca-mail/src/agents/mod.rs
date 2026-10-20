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

use crate::domain::AgentKind;
use crate::error::{AgentError, LlmError};
use crate::llm::LlmResponse;
use crate::orchestration::AgentContext;

/// Common trait for all agents.
#[async_trait]
pub trait Agent: Send + Sync {
    fn kind(&self) -> AgentKind;

    /// Call the LLM and parse a structured response.
    ///
    /// A malformed structured answer is a model hiccup, not a verdict: one
    /// fresh attempt is the difference between a lost email and a lost
    /// request. Transport errors are not retried here — the provider does
    /// that — only an answer that arrived but made no sense.
    async fn call_llm_structured<T: serde::de::DeserializeOwned>(
        &self,
        context: &AgentContext,
        system_prompt: &str,
        user_message: &str,
    ) -> Result<T, AgentError> {
        let mut parse_failure = None;
        for _ in 0..2 {
            let response = self
                .request_llm(context, system_prompt, user_message)
                .await?;
            match parse_structured(self.kind(), response) {
                Ok(value) => return Ok(value),
                Err(err) => {
                    tracing::warn!(
                        agent = self.kind().as_str(),
                        error = %err,
                        "structured response failed to parse; retrying once"
                    );
                    parse_failure = Some(err);
                }
            }
        }
        Err(parse_failure.expect("the loop body always sets the failure"))
    }

    /// Single LLM round trip: metrics, token accounting and error mapping in
    /// one place so every agent call is observed and billed the same way.
    async fn request_llm(
        &self,
        context: &AgentContext,
        system_prompt: &str,
        user_message: &str,
    ) -> Result<LlmResponse, AgentError> {
        use std::time::Instant;

        let corr = crate::observability::Correlation::new(context.email_id, context.run_id);
        let tier = self.kind().tier();
        let agent = self.kind().as_str();
        let provider = context.llm.provider_name();
        let model = context.llm.resolve_model(tier);
        crate::observability::llm::llm_request_started(&corr, agent, provider, &model);
        let started = Instant::now();

        // Open the accounting row before the call so a crash mid-request is
        // visible as a stuck `running` agent run rather than silence.
        let usage_row = usage::begin(context, self.kind(), &model, tier.as_str()).await;

        let response = match context
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
        {
            Ok(response) => response,
            Err(e) => {
                let elapsed = started.elapsed();
                usage::finish_failed(context, usage_row, elapsed, &e).await;
                crate::observability::llm::llm_request_completed(
                    &corr,
                    agent,
                    provider,
                    &model,
                    elapsed.as_millis() as u64,
                    0,
                    0,
                );
                return Err(AgentError::InvalidOutput {
                    agent: self.kind(),
                    reason: e.to_string(),
                });
            }
        };

        crate::observability::llm::llm_request_completed(
            &corr,
            agent,
            provider,
            &model,
            started.elapsed().as_millis() as u64,
            response.prompt_tokens,
            response.completion_tokens,
        );
        usage::finish_ok(context, usage_row, &response, started.elapsed()).await;
        Ok(response)
    }

    fn agent_identity(&self) -> AgentIdentity {
        identity::identity_of(self.kind())
    }
}

/// Parse the model output as JSON, tolerating markdown fences and
/// surrounding prose from models that do not strictly honour the contract.
fn parse_structured<T: serde::de::DeserializeOwned>(
    kind: AgentKind,
    response: LlmResponse,
) -> Result<T, AgentError> {
    let content = response.content.trim();
    let json_str = prompt::extract_json_object(content).unwrap_or(content);
    serde_json::from_str::<T>(json_str).map_err(|e| AgentError::InvalidOutput {
        agent: kind,
        reason: format!("JSON parse error: {e}"),
    })
}

/// Token/cost accounting for LLM calls.
///
/// Every path is non-fatal: accounting failures are logged and dropped, never
/// propagated — losing cost stats is better than losing the email.
mod usage {
    use std::time::Duration;

    use crate::domain::{AgentKind, RunState};
    use crate::llm::LlmResponse;
    use crate::orchestration::AgentContext;
    use crate::persistence::run_repo;

    /// `agent_runs` row opened for one LLM call, if accounting worked.
    type UsageRow = Option<crate::domain::AgentRunId>;

    pub(super) async fn begin(
        context: &AgentContext,
        agent: AgentKind,
        model: &str,
        tier: &str,
    ) -> UsageRow {
        let started = run_repo::start_agent(
            &context.pool,
            context.run_id,
            context.email_id,
            agent,
            model,
            tier,
        )
        .await;
        match started {
            Ok(id) => Some(id),
            Err(e) => {
                warn("start_agent", &e);
                None
            }
        }
    }

    pub(super) async fn finish_ok(
        context: &AgentContext,
        row: UsageRow,
        response: &LlmResponse,
        elapsed: Duration,
    ) {
        let prompt = response.prompt_tokens;
        let completion = response.completion_tokens;
        let cost = context.llm.estimate_cost(prompt, completion);

        if let Some(id) = row {
            finish(
                context,
                id,
                RunState::Succeeded,
                elapsed,
                None,
                prompt,
                completion,
                cost,
            )
            .await;
        }
        // Run totals keep the per-run view and the budget guard honest;
        // `cost_since` reads `agent_runs`, which `finish` just updated.
        if let Err(e) =
            run_repo::add_cost(&context.pool, context.run_id, prompt + completion, cost).await
        {
            warn("add_cost", &e);
        }
    }

    pub(super) async fn finish_failed(
        context: &AgentContext,
        row: UsageRow,
        elapsed: Duration,
        error: &impl std::fmt::Display,
    ) {
        if let Some(id) = row {
            finish(
                context,
                id,
                RunState::Failed,
                elapsed,
                Some(&error.to_string()),
                0,
                0,
                0,
            )
            .await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish(
        context: &AgentContext,
        id: crate::domain::AgentRunId,
        state: RunState,
        elapsed: Duration,
        error: Option<&str>,
        prompt: i64,
        completion: i64,
        cost: i64,
    ) {
        if let Err(e) = run_repo::finish_agent(
            &context.pool,
            id,
            state,
            0,
            0,
            elapsed.as_millis() as i64,
            error,
        )
        .await
        {
            warn("finish_agent", &e);
        }
        if state == RunState::Succeeded || prompt + completion + cost > 0 {
            if let Err(e) =
                run_repo::add_agent_cost(&context.pool, id, prompt, completion, cost).await
            {
                warn("add_agent_cost", &e);
            }
        }
    }

    fn warn(step: &str, error: &impl std::fmt::Display) {
        tracing::warn!(step, error = %error, "token usage accounting failed (continuing)");
    }
}
