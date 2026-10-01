//! Field capture shared by the pretty formatter and the SSE broadcast layer.

use tracing::field::{Field, Visit};
use tracing::Event;

/// One event field value, typed enough for JSON and display.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Val {
    Str(String),
    Int(i64),
    Uint(u64),
    Float(f64),
    Bool(bool),
}

impl Val {
    /// Render for the pretty terminal format (raw, no quotes).
    pub(crate) fn as_display(&self) -> String {
        match self {
            Val::Str(s) => s.clone(),
            Val::Int(i) => i.to_string(),
            Val::Uint(u) => u.to_string(),
            Val::Float(f) => f.to_string(),
            Val::Bool(b) => b.to_string(),
        }
    }

    /// Render as a `serde_json::Value` for SSE payloads.
    pub(crate) fn as_json(&self) -> serde_json::Value {
        match self {
            Val::Str(s) => serde_json::Value::String(s.clone()),
            Val::Int(i) => serde_json::Value::from(*i),
            Val::Uint(u) => serde_json::Value::from(*u),
            Val::Float(f) => serde_json::json!(f),
            Val::Bool(b) => serde_json::Value::from(*b),
        }
    }
}

/// All fields of one tracing event: the message (event name) and the rest.
#[derive(Debug, Default)]
pub(crate) struct Captured {
    pub(crate) message: Option<String>,
    pub(crate) fields: Vec<(String, Val)>,
}

impl Captured {
    /// Record every field of `event` in declaration order.
    pub(crate) fn from_event(event: &Event<'_>) -> Self {
        let mut cap = Captured::default();
        event.record(&mut cap);
        cap
    }

    /// Look up a field by name (used by tests and the HTTP logger).
    pub(crate) fn get(&self, name: &str) -> Option<&Val> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

impl Captured {
    fn push(&mut self, field: &Field, val: Val) {
        // `log.*` fields come from the `log`-crate bridge and only add noise.
        if !field.name().starts_with("log.") {
            self.fields.push((field.name().to_string(), val));
        }
    }
}

impl Visit for Captured {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // `%value` fields and `format_args!` messages arrive here; Debug of
        // both renders the plain display text, so no quoting is applied.
        let text = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(text);
        } else {
            self.push(field, Val::Str(text));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.push(field, Val::Str(value.to_string()));
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field, Val::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field, Val::Int(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field, Val::Uint(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.push(field, Val::Float(value));
    }
}
