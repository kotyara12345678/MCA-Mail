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

/// Cut a string to `max` bytes for a prompt.
///
/// The budget is bytes, because that is what reaches the model, but the slice
/// has to land on a UTF-8 character boundary: a byte index in the middle of a
/// Cyrillic character panics, and a panic here kills the worker thread that is
/// building the prompt. The suffix says how much was cut in characters, which
/// is the unit a human reading the prompt expects.
pub(super) fn truncate_for_prompt(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}... [truncated {} chars]",
        &text[..end],
        text[end..].chars().count()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_whole_characters_at_the_boundary() {
        // Byte 3000 lands inside a Cyrillic letter here, which is exactly
        // where a plain `&s[..3000]` panicked while a spam prompt was being
        // built and took the worker thread with it.
        let text = format!("a{}", "е".repeat(3000));
        let cut = truncate_for_prompt(&text, 3000);
        assert!(
            cut.starts_with("aе"),
            "the cut must stay on a char boundary"
        );
        assert!(cut.ends_with("[truncated 1501 chars]"));
    }

    #[test]
    fn truncate_leaves_short_input_alone() {
        assert_eq!(truncate_for_prompt("коротко", 100), "коротко");
    }

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
