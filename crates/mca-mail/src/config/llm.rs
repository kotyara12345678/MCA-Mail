use serde::{Deserialize, Serialize};

/// Which LLM transport to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderKind {
    /// Any OpenAI-compatible `/chat/completions` endpoint.
    #[default]
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    /// Deterministic in-process responses, for tests and local development.
    Mock,
}

impl LlmProviderKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            LlmProviderKind::OpenAiCompatible => "openai_compatible",
            LlmProviderKind::Mock => "mock",
        }
    }
}

/// Per-agent model overrides.
///
/// The orchestrator picks a model tier per agent; the mapping below lets an
/// operator route tiers to whatever models their provider actually offers
/// without touching code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelRouting {
    pub cheap: String,
    pub standard: String,
    pub capable: String,
}

/// USD price per million tokens, used for the cost estimate metric.
///
/// Deliberately configurable and per-provider so the estimate stays honest when
/// the account's pricing differs from the published list price.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelPrice {
    pub prompt_per_million_usd: f64,
    pub completion_per_million_usd: f64,
}

impl Default for ModelPrice {
    fn default() -> Self {
        Self {
            prompt_per_million_usd: 0.15,
            completion_per_million_usd: 0.60,
        }
    }
}

impl ModelPrice {
    /// Cost in micro-dollars for a call, rounded up so nothing is hidden.
    pub fn cost_micros(&self, prompt_tokens: i64, completion_tokens: i64) -> i64 {
        let prompt = prompt_tokens as f64 / 1_000_000.0 * self.prompt_per_million_usd;
        let completion = completion_tokens as f64 / 1_000_000.0 * self.completion_per_million_usd;
        ((prompt + completion) * 1_000_000.0).ceil() as i64
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmSettings {
    pub provider: LlmProviderKind,
    /// Base URL without the `/chat/completions` suffix.
    pub base_url: String,
    pub api_key: super::Secret,
    /// Default model used when a tier has no explicit override.
    pub model: String,
    pub routing: ModelRouting,
    pub max_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub timeout_seconds: u64,
    pub connect_timeout_seconds: u64,
    pub max_retries: u32,
    /// Initial retry backoff; grows exponentially.
    pub retry_backoff_ms: u64,
    /// Consecutive failures before the circuit breaker opens.
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_cooldown_seconds: u64,
    /// Ask for strict JSON-schema conforming output where supported.
    pub json_mode: bool,
    pub price: ModelPrice,
    /// Refuse to call the provider at all (used by CI and unit tests).
    pub disabled: bool,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            provider: LlmProviderKind::OpenAiCompatible,
            base_url: String::new(),
            api_key: super::Secret::empty(),
            model: String::new(),
            routing: ModelRouting::default(),
            max_tokens: 2048,
            temperature: 0.1,
            top_p: 0.9,
            timeout_seconds: 60,
            connect_timeout_seconds: 10,
            max_retries: 3,
            retry_backoff_ms: 500,
            circuit_breaker_threshold: 5,
            circuit_breaker_cooldown_seconds: 60,
            json_mode: true,
            price: ModelPrice::default(),
            disabled: false,
        }
    }
}

impl LlmSettings {
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout_seconds.max(1))
    }

    pub fn connect_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.connect_timeout_seconds.max(1))
    }

    /// Resolve the model to use for a cost/latency tier.
    pub fn model_for(&self, tier: crate::domain::ModelTier) -> String {
        let routed = match tier {
            crate::domain::ModelTier::Cheap => &self.routing.cheap,
            crate::domain::ModelTier::Standard => &self.routing.standard,
            crate::domain::ModelTier::Capable => &self.routing.capable,
        };
        if routed.trim().is_empty() {
            self.model.clone()
        } else {
            routed.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_routing_falls_back_to_default_model() {
        let settings = LlmSettings {
            model: "default-model".into(),
            routing: ModelRouting {
                cheap: "fast-model".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            settings.model_for(crate::domain::ModelTier::Cheap),
            "fast-model"
        );
        assert_eq!(
            settings.model_for(crate::domain::ModelTier::Capable),
            "default-model"
        );
    }

    #[test]
    fn price_estimate_is_rounded_up_in_micro_dollars() {
        let price = ModelPrice::default();
        assert_eq!(price.cost_micros(0, 0), 0);
        assert_eq!(price.cost_micros(1_000_000, 0), 150_000);
        assert_eq!(price.cost_micros(0, 1_000_000), 600_000);
        assert!(price.cost_micros(1, 1) >= 1);
    }
}
