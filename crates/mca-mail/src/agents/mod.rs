//! AI Agents module — all specialized agent implementations.

mod classification;
mod communication;
mod handoff;
mod logistics;
mod qualification;
mod research;
mod spam;

pub use classification::ClassificationAgent;
pub use communication::CommunicationAgent;
pub use handoff::HandoffAgent;
pub use logistics::LogisticsExpertAgent;
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
    async fn call_llm_structured<T: serde::de::DeserializeOwned>(
        &self,
        context: &super::orchestration::AgentContext,
        system_prompt: &str,
        user_message: &str,
    ) -> Result<T, AgentError> {
        let response = context
            .llm
            .chat_for_tier(
                self.kind().tier(),
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

        // Try to parse as JSON, tolerating markdown fences and surrounding
        // prose from models that do not strictly honour the response contract.
        let content = response.content.trim();
        let json_str = extract_json_object(content).unwrap_or(content);

        serde_json::from_str::<T>(json_str).map_err(|e| AgentError::InvalidOutput {
            agent: self.kind(),
            reason: format!(
                "JSON parse error: {e}. Raw: {}",
                truncate_utf8(&response.content, 300)
            ),
        })
    }

    fn agent_identity(&self) -> AgentIdentity {
        match self.kind() {
            AgentKind::Spam => AgentIdentity {
                name: "Spam Agent",
                description: "I identify spam, advertisements, phishing, and automated notifications.",
            },
            AgentKind::Classification => AgentIdentity {
                name: "Classification Agent",
                description: "I classify inbound emails into categories like new lead, existing client, or transport request.",
            },
            AgentKind::LeadQualification => AgentIdentity {
                name: "Lead Qualification Agent",
                description: "I extract commercial details from potential client emails.",
            },
            AgentKind::LogisticsExpert => AgentIdentity {
                name: "Logistics Expert Agent",
                description: "I advise on international logistics, customs, and trade processes.",
            },
            AgentKind::CompanyResearch => AgentIdentity {
                name: "Company Research Agent",
                description: "I check company details from public registries.",
            },
            AgentKind::EmailCommunication => AgentIdentity {
                name: "Email Communication Agent",
                description: "I draft professional replies to potential clients.",
            },
            AgentKind::Handoff => AgentIdentity {
                name: "Handoff Agent",
                description: "I prepare structured lead handoffs for human managers.",
            },
        }
    }
}

pub struct AgentIdentity {
    pub name: &'static str,
    pub description: &'static str,
}

/// Build the tool-use system prompt for an agent.
pub fn tool_use_prompt(tools: &[crate::tools::ToolDef]) -> String {
    if tools.is_empty() {
        return String::new();
    }

    let mut prompt = String::from("\n\nYou have access to the following tools:\n");
    for tool in tools {
        prompt.push_str(&format!("\n- `{}`: {}", tool.name, tool.description));
    }
    prompt.push_str("\n\nWhen you need to use a tool, respond with a JSON object containing `tool` and `args` fields. After calling a tool you will receive the result and must continue your analysis.");
    prompt
}

/// Extract the first balanced JSON object from arbitrary text.
///
/// Handles markdown fences and conversational prose around the object, which
/// some models emit despite a strict "respond with JSON" instruction.
fn extract_json_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'"' if !escaped => in_string = !in_string,
            b'\\' if in_string => escaped = !escaped,
            _ => escaped = false,
        }
        if !in_string {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&text[start..=i]);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Truncate a string to `max` bytes without splitting a UTF-8 character.
fn truncate_utf8(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_fenced_markdown() {
        let text = "Here you go:\n```json\n{\"a\": 1}\n```";
        assert_eq!(extract_json_object(text), Some("{\"a\": 1}"));
    }

    #[test]
    fn extracts_json_from_surrounding_prose() {
        let text = "I'll process this. {\"b\": 2} Thank you!";
        assert_eq!(extract_json_object(text), Some("{\"b\": 2}"));
    }

    #[test]
    fn handles_nested_objects_and_strings() {
        let text = r#"{"a": {"b": "}", "c": [1, { "d": "{" }]}}"#;
        assert_eq!(extract_json_object(text), Some(text));
    }

    #[test]
    fn returns_none_without_braces() {
        assert_eq!(extract_json_object("just text"), None);
    }
}
