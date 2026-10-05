//! Server-side validation of what the qualification model produced.
//!
//! The model's JSON is a *claim*, never a fact: this module is the gate every
//! extracted value passes on its way into `lead_requirements`. It answers four
//! questions the model cannot be trusted to answer itself — is the field one
//! we actually store, is the state one we actually allow, is the value within
//! bounds, and did the same field arrive twice with two different answers.

use serde::{Deserialize, Serialize};

use super::super::class::FieldState;
use super::super::ids::LeadId;
use super::{LeadRequirement, RequirementField, RequirementSource};

/// Hard cap on a stored value; anything longer is truncated rather than
/// rejected, because a long answer is still an answer.
const MAX_VALUE_CHARS: usize = 500;
const MAX_UNIT_CHARS: usize = 32;
const MAX_EVIDENCE_CHARS: usize = 300;

/// One requirement exactly as the model emitted it. Every field is optional or
/// a string so a malformed row can be dropped instead of failing the parse of
/// the whole response — and a scalar of the wrong type is coerced rather than
/// rejected, because one `"value": true` must not cost us the whole batch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RawRequirement {
    #[serde(deserialize_with = "super::super::flex::string")]
    pub field: String,
    #[serde(default, deserialize_with = "super::super::flex::opt_string")]
    pub value: Option<String>,
    #[serde(default, deserialize_with = "super::super::flex::opt_string")]
    pub state: Option<String>,
    #[serde(default, deserialize_with = "super::super::flex::opt_string")]
    pub unit: Option<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default, deserialize_with = "super::super::flex::opt_string")]
    pub evidence: Option<String>,
}

/// What happened to the batch, so the caller can record it instead of guessing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NormalizeReport {
    /// Rows that will be written.
    pub accepted: usize,
    /// Field names the model used that are not part of our closed set.
    pub unknown_fields: Vec<String>,
    /// States the model used that are not part of our closed set.
    pub invalid_states: Vec<String>,
    /// Fields that appeared more than once; only the strongest survives.
    pub merged_fields: Vec<String>,
}

impl NormalizeReport {
    pub fn is_clean(&self) -> bool {
        self.unknown_fields.is_empty() && self.invalid_states.is_empty()
    }
}

/// Validate, normalise, de-duplicate and bound a model-extracted batch.
///
/// The rules, in order:
/// * an unrecognised field is dropped, never stored under a guessed name;
/// * an empty value is stored as `unknown`, not as `""`;
/// * `known` is only accepted with a verbatim evidence span, and then it is
///   recorded as `customer` — otherwise the model would be promoting its own
///   guess into a fact the CRM treats as confirmed;
/// * `not_applicable` is accepted as declared; every unrecognised or
///   unsupported state becomes `needs_confirmation` with `ai_inference`, so a
///   later customer statement always wins in `requirement_repo::upsert_many`;
/// * duplicates collapse onto the strongest answer rather than last-write-wins.
pub fn normalize(
    lead_id: LeadId,
    raw: &[RawRequirement],
) -> (Vec<LeadRequirement>, NormalizeReport) {
    let mut report = NormalizeReport::default();
    let mut seen: Vec<LeadRequirement> = Vec::new();

    for item in raw {
        let Ok(field) = item.field.parse::<RequirementField>() else {
            report
                .unknown_fields
                .push(trim_to(&item.field, MAX_UNIT_CHARS));
            continue;
        };

        let value = item
            .value
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let evidence = item
            .evidence
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| trim_to(v, MAX_EVIDENCE_CHARS));
        let state = resolve_state(
            item.state.as_deref(),
            value,
            evidence.as_deref(),
            &mut report,
        );
        let confidence = item
            .confidence
            .map(|c| c.clamp(0.0, 1.0))
            .filter(|_| state != FieldState::Unknown && state != FieldState::NotApplicable);

        let requirement = LeadRequirement {
            lead_id,
            field,
            value: value.map(|v| trim_to(v, MAX_VALUE_CHARS)),
            state,
            source: source_of(state),
            unit: item
                .unit
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .map(|u| trim_to(u, MAX_UNIT_CHARS)),
            confidence,
            evidence: if state == FieldState::Known {
                evidence
            } else {
                None
            },
            updated_at: chrono::Utc::now(),
        };

        match seen.iter_mut().find(|existing| existing.field == field) {
            Some(existing) => {
                report.merged_fields.push(field.as_str().to_string());
                if requirement.precedence() > existing.precedence() {
                    *existing = requirement;
                }
            }
            None => seen.push(requirement),
        }
    }

    report.accepted = seen.len();
    (seen, report)
}

/// Decide the knowledge state from what the model claimed.
///
/// `known` without evidence is the interesting case: the model is asserting a
/// fact it cannot point to, so it is downgraded rather than believed.
fn resolve_state(
    claimed: Option<&str>,
    value: Option<&str>,
    evidence: Option<&str>,
    report: &mut NormalizeReport,
) -> FieldState {
    let Some(claimed) = claimed.map(str::trim).filter(|s| !s.is_empty()) else {
        return default_state(value, evidence);
    };
    match claimed.parse::<FieldState>() {
        Ok(FieldState::Known) if value.is_some() && evidence.is_some() => FieldState::Known,
        Ok(FieldState::Known) => FieldState::NeedsConfirmation,
        Ok(other) => other,
        Err(_) => {
            report.invalid_states.push(trim_to(claimed, MAX_UNIT_CHARS));
            default_state(value, evidence)
        }
    }
}

/// What to believe when the model said nothing: an empty answer is no answer.
fn default_state(value: Option<&str>, evidence: Option<&str>) -> FieldState {
    match (value, evidence) {
        (Some(_), Some(_)) => FieldState::Known,
        (Some(_), None) => FieldState::NeedsConfirmation,
        (None, _) => FieldState::Unknown,
    }
}

const fn source_of(state: FieldState) -> RequirementSource {
    match state {
        FieldState::Known => RequirementSource::Customer,
        _ => RequirementSource::AiInference,
    }
}

fn trim_to(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let cut: String = value.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
#[path = "normalize_tests.rs"]
mod tests;
