//! Prompt construction and response-parsing helpers shared by all agents.

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
pub(super) fn extract_json_object(text: &str) -> Option<&str> {
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
pub(super) fn truncate_utf8(text: &str, max: usize) -> String {
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
