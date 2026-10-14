//! OpenAI-compatible /chat/completions provider.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{LlmMessage, LlmProvider, LlmRequest, LlmResponse, LlmRole};
use crate::config::LlmSettings;
use crate::domain::ModelTier;
use crate::error::LlmError;

pub struct OpenAiProvider {
    base_url: String,
    api_key: String,
    default_model: String,
    routing: crate::config::ModelRouting,
    max_tokens: u32,
    temperature: f32,
    max_retries: u32,
    retry_backoff_ms: u64,
    price: crate::config::ModelPrice,
    timeout_seconds: u64,
    json_mode: bool,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(settings: &LlmSettings) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(
                settings.timeout_seconds.max(1),
            ))
            .connect_timeout(std::time::Duration::from_secs(
                settings.connect_timeout_seconds.max(1),
            ))
            .build()
            .unwrap_or_default();

        OpenAiProvider {
            base_url: settings.base_url.trim_end_matches('/').to_string(),
            api_key: settings.api_key.expose_owned(),
            default_model: settings.model.clone(),
            routing: settings.routing.clone(),
            max_tokens: settings.max_tokens.max(1),
            temperature: settings.temperature,
            max_retries: settings.max_retries,
            retry_backoff_ms: settings.retry_backoff_ms.max(100),
            price: settings.price,
            timeout_seconds: settings.timeout_seconds,
            json_mode: settings.json_mode,
            client,
        }
    }

    /// Assemble the wire request from the tier request: system prompt folded,
    /// max_tokens defaulting to the configured cap, JSON mode when enabled.
    fn build_request(&self, req: &LlmRequest) -> ChatRequest {
        let (system_content, messages) = wire_messages(req);
        let mut api_messages = Vec::with_capacity(messages.len() + 1);
        if !system_content.is_empty() {
            api_messages.push(Message {
                role: "system".into(),
                content: system_content,
            });
        }
        api_messages.extend(messages);

        let mut chat_req = ChatRequest {
            model: req.model.clone(),
            messages: api_messages,
            max_tokens: if req.max_tokens > 0 {
                req.max_tokens
            } else {
                self.max_tokens
            },
            temperature: req.temperature,
            response_format: None,
        };
        if self.json_mode && req.response_schema.is_some() {
            chat_req.response_format = Some(ResponseFormat {
                type_: "json_object".into(),
            });
        }
        chat_req
    }

    /// Perform exactly one HTTP call and classify the outcome for `chat`.
    async fn send_attempt(&self, url: &str, chat_req: &ChatRequest) -> Attempt {
        let response = match self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(chat_req)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return Attempt::Retryable(transport_error(error, self.timeout_seconds));
            }
        };

        let status = response.status();
        if status.is_success() {
            return match response.json::<ChatResponse>().await {
                Ok(data) => Attempt::Success(data),
                Err(error) => {
                    Attempt::Retryable(LlmError::Malformed(format!("JSON parse: {error}")))
                }
            };
        }

        // 429 and 5xx are transient; every other status is the caller's fault
        // (bad key, bad request) and will not improve on retry.
        let body = response.text().await.unwrap_or_default();
        let error = LlmError::Http {
            status: status.as_u16(),
            detail: body,
        };
        if status.as_u16() == 429 || status.as_u16() >= 500 {
            Attempt::Retryable(error)
        } else {
            Attempt::Fatal(error)
        }
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: i64,
    completion_tokens: i64,
}

/// Emit `llm_failed` for a final transport failure (never logs prompts).
fn report_llm_failed(provider: &OpenAiProvider, model: &str, err: &LlmError, retry_count: u32) {
    crate::observability::llm::llm_failed(
        provider.provider_name(),
        model,
        crate::observability::errors::llm_error_type(err),
        retry_count,
        &err.to_string(),
    );
}

/// Fold `req.system_prompt` with a leading `system` message (if any) into one
/// system prompt, and convert the remaining messages to the wire shape.
fn wire_messages(req: &LlmRequest) -> (String, Vec<Message>) {
    let mut system_content = req.system_prompt.clone();
    let mut messages = Vec::with_capacity(req.messages.len());
    if let Some(first) = req.messages.first() {
        if first.role == LlmRole::System {
            system_content = format!("{}\n\n{}", system_content, first.content);
            messages.extend(req.messages.iter().skip(1));
        } else {
            messages.extend(req.messages.iter());
        }
    }
    let api_messages = messages
        .into_iter()
        .map(|m| Message {
            role: role_str(m.role),
            content: m.content.clone(),
        })
        .collect();
    (system_content, api_messages)
}

/// One HTTP attempt, classified for the retry loop: a parsed body, an error
/// worth retrying, or a client-side error that fails immediately.
enum Attempt {
    Success(ChatResponse),
    Retryable(LlmError),
    Fatal(LlmError),
}

/// Map a `reqwest` transport failure onto the error taxonomy.
fn transport_error(error: reqwest::Error, timeout_seconds: u64) -> LlmError {
    if error.is_timeout() {
        LlmError::Timeout(timeout_seconds * 1000)
    } else if error.is_connect() {
        LlmError::Unavailable(error.to_string())
    } else {
        LlmError::Internal(error.to_string())
    }
}

/// Turn a successful API body into the response the orchestrator sees.
fn parse_success(data: ChatResponse, model: &str) -> LlmResponse {
    let content = data
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default();
    let (prompt_tokens, completion_tokens) = data
        .usage
        .map(|u| (u.prompt_tokens, u.completion_tokens))
        .unwrap_or((0, 0));
    LlmResponse {
        content,
        prompt_tokens,
        completion_tokens,
        model: model.to_string(),
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn chat(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/chat/completions", self.base_url);
        let chat_req = self.build_request(&req);
        let max_attempts = (self.max_retries + 1) as usize;
        let mut last_error: Option<LlmError> = None;

        for attempt in 0..max_attempts {
            if attempt > 0 {
                let cause = last_error
                    .as_ref()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "retryable failure".into());
                crate::observability::llm::llm_retry(
                    self.provider_name(),
                    &chat_req.model,
                    attempt as u32,
                    &cause,
                );
                let backoff = self.retry_backoff_ms * 2u64.pow(attempt as u32 - 1);
                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
            }

            let final_attempt = attempt == max_attempts - 1;
            match self.send_attempt(&url, &chat_req).await {
                Attempt::Success(data) => return Ok(parse_success(data, &req.model)),
                Attempt::Fatal(err) => {
                    report_llm_failed(self, &chat_req.model, &err, attempt as u32);
                    return Err(err);
                }
                Attempt::Retryable(err) => {
                    if final_attempt {
                        report_llm_failed(self, &chat_req.model, &err, attempt as u32);
                        return Err(err);
                    }
                    last_error = Some(err);
                }
            }
        }

        let err = last_error.unwrap_or(LlmError::Internal("max retries exceeded".into()));
        report_llm_failed(
            self,
            &chat_req.model,
            &err,
            max_attempts.saturating_sub(1) as u32,
        );
        Err(err)
    }

    fn resolve_model(&self, tier: ModelTier) -> String {
        let routed = match tier {
            ModelTier::Cheap => &self.routing.cheap,
            ModelTier::Standard => &self.routing.standard,
            ModelTier::Capable => &self.routing.capable,
        };
        if routed.trim().is_empty() {
            self.default_model.clone()
        } else {
            routed.clone()
        }
    }

    fn default_temperature(&self) -> f32 {
        self.temperature
    }

    fn provider_name(&self) -> &'static str {
        "openai_compatible"
    }

    fn estimate_cost(&self, prompt: i64, completion: i64) -> i64 {
        self.price.cost_micros(prompt, completion)
    }
}

fn role_str(role: LlmRole) -> String {
    match role {
        LlmRole::System => "system".into(),
        LlmRole::User => "user".into(),
        LlmRole::Assistant => "assistant".into(),
    }
}
