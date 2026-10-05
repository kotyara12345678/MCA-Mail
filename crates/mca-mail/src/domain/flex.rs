//! Coercion for fields a model may answer with the wrong scalar type.
//!
//! A structured prompt asks for `"company_name": null` and a model answers
//! `"company_name": true` because the question read as yes/no, or emits
//! `"value": 12` for a number. Rejecting the whole turn for that throws away
//! an otherwise good extraction and parks the email for human review — the
//! one outcome a wrong scalar type should never cause. So any scalar is
//! accepted where a string is expected and rendered as its JSON text; the
//! real validation still happens downstream, where a value that does not
//! belong is dropped rather than trusted.

use serde::{Deserialize, Deserializer};

/// `Some("x")` for a string, `Some("true")`/`Some("12")` for other scalars,
/// `None` for `null` or a missing field.
pub fn opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.map(render))
}

/// Like [`opt_string`], for a field the schema requires.
pub fn string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(opt_string(deserializer)?.unwrap_or_default())
}

/// A list whose elements may each have arrived as any scalar.
pub fn string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Option::<Vec<serde_json::Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(values.into_iter().map(render).collect())
}

fn render(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text,
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Row {
        #[serde(default, deserialize_with = "opt_string")]
        value: Option<String>,
        #[serde(deserialize_with = "string")]
        field: String,
        #[serde(default, deserialize_with = "string_list")]
        questions: Vec<String>,
    }

    #[test]
    fn a_boolean_becomes_the_word_the_model_meant() {
        let row: Row = serde_json::from_str(
            r#"{"value": true, "field": "needs_insurance", "questions": [true, "вес?"]}"#,
        )
        .expect("a boolean where a string was asked for must still parse");
        assert_eq!(row.value.as_deref(), Some("true"));
        assert_eq!(row.field, "needs_insurance");
        assert_eq!(row.questions, vec!["true".to_string(), "вес?".to_string()]);
    }

    #[test]
    fn numbers_and_null_keep_their_meaning() {
        let row: Row =
            serde_json::from_str(r#"{"value": 12000, "field": "goods_weight", "questions": []}"#)
                .expect("a number where a string was asked for must still parse");
        assert_eq!(row.value.as_deref(), Some("12000"));

        let row: Row =
            serde_json::from_str(r#"{"value": null, "field": "goods_name", "questions": []}"#)
                .expect("null must stay null, not become an empty string");
        assert_eq!(row.value, None);
    }

    #[test]
    fn a_plain_string_passes_through_unchanged() {
        let row: Row = serde_json::from_str(
            r#"{"value": "станки", "field": "goods_name", "questions": ["вес?"]}"#,
        )
        .expect("the normal shape must parse");
        assert_eq!(row.value.as_deref(), Some("станки"));
    }
}
