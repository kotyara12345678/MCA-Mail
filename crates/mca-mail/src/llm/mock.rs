//! Deterministic mock LLM provider for tests and local development.
//!
//! Returns canned responses based on the last user message content.
//! This avoids any real API calls and keeps tests reproducible.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::{LlmProvider, LlmRequest, LlmResponse};
use crate::domain::ModelTier;
use crate::error::LlmError;

struct Inner {
    /// Map from message content prefix -> canned response.
    responses: HashMap<String, String>,
    /// Track calls for assertions.
    call_log: Vec<String>,
}

impl Inner {
    /// Longest matching prefix wins, so a catch-all `""` stub only applies
    /// when nothing more specific matches. HashMap iteration order must not
    /// decide which canned answer a test gets.
    fn resolve(&self, content: &str) -> Option<String> {
        self.responses
            .iter()
            .filter(|(prefix, _)| prefix.is_empty() || content.contains(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, response)| response.clone())
    }
}

pub struct MockLlmProvider {
    inner: Mutex<Inner>,
    default_model: String,
    temperature: f32,
    price: crate::config::ModelPrice,
}

impl MockLlmProvider {
    pub fn new(settings: &crate::config::LlmSettings) -> Self {
        MockLlmProvider {
            inner: Mutex::new(Inner {
                responses: HashMap::new(),
                call_log: Vec::new(),
            }),
            default_model: if settings.model.is_empty() {
                "mock-model".into()
            } else {
                settings.model.clone()
            },
            temperature: settings.temperature,
            price: settings.price,
        }
    }

    /// Register a canned response triggered when user message contains `prefix`.
    pub fn stub(&self, prefix: &str, response: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner
            .responses
            .insert(prefix.to_string(), response.to_string());
    }

    /// Stub a structured JSON response.
    pub fn stub_json(&self, prefix: &str, value: &impl serde::Serialize) {
        let json = serde_json::to_string(value).unwrap_or_default();
        self.stub(prefix, &json);
    }

    /// Return a list of all messages passed to chat (concatenated user content).
    #[allow(dead_code)]
    pub fn call_log(&self) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        inner.call_log.clone()
    }

    /// Clear all stubs and call log.
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.responses.clear();
        inner.call_log.clear();
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn chat(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        // Stubs are keyed on prompt content, and agents put their identity in
        // the system prompt, so it must participate in matching.
        let mut body = format!("system: {}", req.system_prompt);
        for m in &req.messages {
            body.push_str(&format!("\n{}: {}", role_label(m.role), m.content));
        }

        let response = {
            let mut inner = self.inner.lock().unwrap();
            inner.call_log.push(body.clone());
            inner.resolve(&body).or_else(|| {
                // Try each message individually as fallback
                for msg in &req.messages {
                    if let Some(r) = inner.resolve(&msg.content) {
                        return Some(r);
                    }
                }
                None
            })
        };

        match response {
            Some(content) => {
                let tokens = content.len() as i64 / 4;
                Ok(LlmResponse {
                    content,
                    prompt_tokens: tokens / 2,
                    completion_tokens: tokens,
                    model: req.model,
                })
            }
            None => Ok(LlmResponse {
                content: r#"{"error":"no stub matched","hint":"register a stub with prefix matching your test input"}"#.into(),
                prompt_tokens: 10,
                completion_tokens: 10,
                model: req.model,
            }),
        }
    }

    fn resolve_model(&self, _tier: ModelTier) -> String {
        self.default_model.clone()
    }

    fn default_temperature(&self) -> f32 {
        self.temperature
    }

    fn provider_name(&self) -> &'static str {
        "mock"
    }

    fn estimate_cost(&self, prompt: i64, completion: i64) -> i64 {
        self.price.cost_micros(prompt, completion)
    }
}

fn role_label(role: super::LlmRole) -> &'static str {
    match role {
        super::LlmRole::System => "system",
        super::LlmRole::User => "user",
        super::LlmRole::Assistant => "assistant",
    }
}
