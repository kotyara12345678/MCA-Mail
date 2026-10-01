//! Unified LLM provider interface.
//!
//! Every agent talks to the model through this trait. Concrete implementations
//! handle transport, retries, circuit breaking, and cost tracking.

pub mod mock;
pub mod openai;

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::ModelTier;
use crate::error::LlmError;

/// Structured request sent to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub model: String,
    pub system_prompt: String,
    pub messages: Vec<LlmMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    /// If set, the provider asks for JSON conforming to this schema.
    pub response_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct LlmUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_micros_usd: i64,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;

    /// Resolve model name for a tier, then call `chat`.
    async fn chat_for_tier(
        &self,
        tier: ModelTier,
        system: &str,
        messages: Vec<LlmMessage>,
        max_tokens: u32,
    ) -> Result<LlmResponse, LlmError> {
        let model = self.resolve_model(tier);
        let req = LlmRequest {
            model,
            system_prompt: system.to_string(),
            messages,
            max_tokens,
            temperature: self.default_temperature(),
            top_p: 0.9,
            response_schema: None,
        };
        self.chat(req).await
    }

    /// Resolve model name for a tier.
    fn resolve_model(&self, tier: ModelTier) -> String;

    fn default_temperature(&self) -> f32;

    fn provider_name(&self) -> &'static str;

    /// Track cost in micro-dollars. Default impl uses config prices.
    fn estimate_cost(&self, prompt: i64, completion: i64) -> i64;
}

/// Shared circuit breaker state.
#[derive(Debug, Clone)]
pub struct CircuitState {
    pub consecutive_failures: u32,
    pub open_until: Option<tokio::time::Instant>,
}

impl CircuitState {
    pub fn is_open(&self) -> bool {
        match self.open_until {
            Some(until) => tokio::time::Instant::now() < until,
            None => false,
        }
    }
}

/// Thread-safe handle to a concrete provider.
pub type ProviderHandle = Arc<dyn LlmProvider>;

/// Factory: build a provider from settings.
pub fn build(settings: &crate::config::LlmSettings) -> Result<ProviderHandle, LlmError> {
    match settings.provider {
        crate::config::LlmProviderKind::Mock => Ok(Arc::new(mock::MockLlmProvider::new(settings))),
        crate::config::LlmProviderKind::OpenAiCompatible => {
            if settings.disabled {
                return Err(LlmError::Unavailable(
                    "LLM provider is disabled by configuration".into(),
                ));
            }
            Ok(Arc::new(openai::OpenAiProvider::new(settings)))
        }
    }
}
