use serde_json::{Map, Number, Value};

pub fn build() -> Value {
    let mut root = Map::new();
    for (flat, nested) in super::env::all_pairs() {
        let Ok(raw) = std::env::var(flat) else {
            continue;
        };
        let Some(value) = env_value(flat, &raw) else {
            continue;
        };
        insert_path(&mut root, nested, value);
    }
    Value::Object(root)
}

fn env_value(flat: &str, raw: &str) -> Option<Value> {
    if raw.trim().is_empty() && flat != "MAIL_MODE" {
        None
    } else {
        Some(parse_value(raw))
    }
}

fn parse_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(number) = trimmed.parse::<i64>() {
        return Value::Number(number.into());
    }
    if let Ok(float) = trimmed.parse::<f64>() {
        if let Some(number) = Number::from_f64(float) {
            return Value::Number(number);
        }
    }
    Value::String(raw.to_string())
}

fn insert_path(root: &mut Map<String, Value>, path: &str, value: Value) {
    let mut current = root;
    let parts: Vec<_> = path.split('.').collect();
    for (index, part) in parts.iter().enumerate() {
        if index == parts.len() - 1 {
            current.insert((*part).into(), value);
            return;
        }
        let entry = current
            .entry(*part)
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        current = entry.as_object_mut().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_mode_overrides_a_file_value_and_fails_typed_parsing() {
        let value = env_value("MAIL_MODE", " ").expect("mode must override file value");
        assert!(serde_json::from_value::<super::super::MailMode>(value).is_err());
        assert!(env_value("MAIL_PROVIDER", " ").is_none());
    }
}
