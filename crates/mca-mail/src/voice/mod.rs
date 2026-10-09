//! Voice-channel helpers shared by the `/api/v1/voice/*` endpoints.
//!
//! Everything here is free of SQL and free of LLM dependencies: it is the
//! server-side authority for what a spoken utterance may write into the CRM.
//! The call agent only proposes values; existence, format and numeric sanity
//! are decided here so a hallucinated "минус десять" never becomes a negative
//! weight in `lead_requirements`.

pub mod fields;
pub mod handoff;
pub mod order_card;
pub mod phone;

use crate::domain::{
    normalize_question, ConversationDirection, ConversationEntry, RequirementField,
    RequirementScope,
};
use crate::error::AppError;

/// One requirement submitted by the voice layer.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InputRequirement {
    pub field: String,
    pub value: String,
    #[serde(default)]
    pub unit: Option<String>,
}

/// A requirement after server-side validation, ready for persistence.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedRequirement {
    pub field: RequirementField,
    pub value: String,
    pub unit: Option<String>,
}

impl InputRequirement {
    /// Validate the wire shape. Returns `Err` for an unknown field, an
    /// over-long value, or a negative / non-numeric value on a field that must
    /// be a positive number.
    pub fn validate(&self) -> Result<ValidatedRequirement, AppError> {
        let field: RequirementField =
            self.field
                .trim()
                .parse()
                .map_err(|e: crate::domain::WireParseError| {
                    AppError::invalid(format!("unknown requirement field: {}", e.value))
                })?;
        let value = fields::sanitize(field, &self.value)?;
        let unit = self
            .unit
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(str::to_string);
        Ok(ValidatedRequirement { field, value, unit })
    }
}

/// The scope a voice lead assumes when the caller does not specify one.
pub const DEFAULT_SCOPE: RequirementScope = RequirementScope::FullImport;

/// Validate a scope string supplied on the wire.
pub fn parse_scope(raw: &str) -> Result<RequirementScope, AppError> {
    raw.parse().map_err(|e: crate::domain::WireParseError| {
        AppError::invalid(format!("unknown scope: {}", e.value))
    })
}

/// One question the agent has already put to this caller, and whether the
/// customer answered it.
///
/// Served on `GET /api/v1/voice/leads/{id}/requirements`, the same endpoint
/// `mca_get_requirements` calls: the model sees its own closed questions and
/// is told not to ask them again, which is how the email pipeline stops turn
/// two from repeating turn one (`orchestration::Dialogue`), applied here to
/// spoken dialogue.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AskedQuestion {
    pub question: String,
    pub answered: bool,
}

/// Questions asked so far, derived from the stored dialogue.
///
/// `history` arrives newest-first (as `voice_repo::conversation` returns it);
/// the walk is chronological. A question is an assistant utterance containing
/// `?` — STT punctuates, and non-question utterances (greetings, closings)
/// are not carried back to the model. Re-asked questions collapse on the
/// same normalized key the email suppression uses, keeping `answered` if the
/// question was ever answered.
pub fn asked_questions(history: &[ConversationEntry]) -> Vec<AskedQuestion> {
    const MAX_QUESTIONS: usize = 30;

    let total_inbound = history
        .iter()
        .filter(|e| matches!(e.direction, ConversationDirection::Inbound))
        .count();
    let mut inbound_before = 0usize;
    let mut asked: Vec<AskedQuestion> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for entry in history.iter().rev() {
        if matches!(entry.direction, ConversationDirection::Inbound) {
            inbound_before += 1;
            continue;
        }
        let body = entry.body.trim();
        if !body.contains('?') {
            continue;
        }
        let key = normalize_question(body);
        if key.is_empty() {
            continue;
        }
        // Every inbound not counted yet sits strictly after this entry, so
        // the question was answered if any of them exists.
        let answered = total_inbound > inbound_before;
        if let Some(pos) = seen.iter().position(|k| k == &key) {
            if answered {
                asked[pos].answered = true;
            }
        } else if asked.len() < MAX_QUESTIONS {
            seen.push(key);
            asked.push(AskedQuestion {
                question: body.to_string(),
                answered,
            });
        }
    }
    asked
}

/// Is this string a plausible email address?
///
/// Deliberately shallow: one `@`, a non-empty local part, a dotted domain with
/// no spaces. Deep RFC 5322 parsing would reject addresses real mail servers
/// accept, and the outbound policy still refuses anything undeliverable.
pub fn is_email(value: &str) -> bool {
    if value.is_empty() || value.len() > 254 || value.split('@').count() != 2 {
        return false;
    }
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !local.contains(' ')
        && !domain.is_empty()
        && !domain.contains(' ')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

/// Validate and normalize a customer email spoken on the call: trimmed and
/// lowercased, so the same dictation always lands on the same recipient and
/// the outbox idempotency key is stable.
pub fn normalize_email(raw: &str) -> Result<String, AppError> {
    let value = raw.trim().to_ascii_lowercase();
    if !is_email(&value) {
        return Err(AppError::invalid("некорректный адрес электронной почты"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scope_is_full_import() {
        assert_eq!(DEFAULT_SCOPE.as_str(), "full_import");
    }

    #[test]
    fn scope_parses_case_insensitively() {
        assert_eq!(parse_scope("Transport").unwrap().as_str(), "transport");
        assert_eq!(parse_scope("full_import").unwrap().as_str(), "full_import");
    }

    #[test]
    fn scope_rejects_garbage() {
        assert!(parse_scope("by_plane").is_err());
    }

    #[test]
    fn requirement_validates_field_and_value() {
        let ok = InputRequirement {
            field: "goods_weight".into(),
            value: "1200".into(),
            unit: Some("кг".into()),
        }
        .validate()
        .unwrap();
        assert_eq!(ok.field.as_str(), "goods_weight");
        assert_eq!(ok.value, "1200");
        assert_eq!(ok.unit.as_deref(), Some("кг"));
    }

    #[test]
    fn requirement_rejects_unknown_field() {
        let bad = InputRequirement {
            field: "horsepower".into(),
            value: "1".into(),
            unit: None,
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn requirement_rejects_negative_numeric() {
        let bad = InputRequirement {
            field: "goods_value".into(),
            value: "-1".into(),
            unit: None,
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn email_accepts_an_ordinary_address() {
        assert!(is_email("buyer@acme.test"));
        assert_eq!(
            normalize_email("  Buyer@Acme.TEST ").unwrap(),
            "buyer@acme.test"
        );
    }

    #[test]
    fn email_rejects_phone_numbers_and_malformed_input() {
        // A voice lead's contact_email holds the caller number until the
        // customer dictates a real address — that must never pass.
        for bad in [
            "",
            "79161234567",
            "a@b",
            "a b@x.com",
            "@x.com",
            "a@",
            "a@.com",
            "a@x.",
        ] {
            assert!(!is_email(bad), "{bad} should not be an email");
            assert!(normalize_email(bad).is_err(), "{bad} should be rejected");
        }
        let huge = format!("{}@x.com", "a".repeat(300));
        assert!(!is_email(&huge));
    }

    fn utterance(direction: ConversationDirection, body: &str) -> ConversationEntry {
        ConversationEntry {
            id: uuid::Uuid::new_v4(),
            lead_id: uuid::Uuid::new_v4(),
            email_id: None,
            direction,
            state: crate::domain::OutboundState::Approved,
            subject: String::new(),
            body: body.to_string(),
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            sent_at: None,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn asked_questions_are_chronological_and_marked_answered() {
        // Stored newest-first: the latest question first.
        let history = vec![
            utterance(ConversationDirection::Outbound, "Куда везём?"),
            utterance(ConversationDirection::Inbound, "Везём запчасти в Казань"),
            utterance(ConversationDirection::Outbound, "Сколько груза?"),
        ];
        assert_eq!(
            asked_questions(&history),
            vec![
                AskedQuestion {
                    question: "Сколько груза?".into(),
                    answered: true,
                },
                AskedQuestion {
                    question: "Куда везём?".into(),
                    answered: false,
                },
            ]
        );
    }

    #[test]
    fn asked_questions_filter_non_questions_and_dedup() {
        let history = vec![
            utterance(ConversationDirection::Outbound, "Из какого города?"),
            utterance(ConversationDirection::Inbound, "Да, 5 тонн"),
            utterance(ConversationDirection::Outbound, "есть вес груза?"),
            utterance(ConversationDirection::Outbound, "Есть вес груза?"),
            utterance(ConversationDirection::Outbound, "Здравствуйте, записываю."),
        ];
        assert_eq!(
            asked_questions(&history),
            vec![
                AskedQuestion {
                    question: "Есть вес груза?".into(),
                    answered: true,
                },
                AskedQuestion {
                    question: "Из какого города?".into(),
                    answered: false,
                },
            ]
        );
    }

    #[test]
    fn asked_questions_empty_history_and_cap() {
        assert!(asked_questions(&[]).is_empty());

        let mut history = Vec::new();
        // Newest-first, as `voice_repo::conversation` returns it: Q39 is the
        // most recent utterance.
        for i in (0..40).rev() {
            history.push(utterance(
                ConversationDirection::Outbound,
                &format!("Вопрос номер {i}?"),
            ));
        }
        // The walk is chronological, capped at 30 questions.
        assert_eq!(asked_questions(&history).len(), 30);
        assert_eq!(asked_questions(&history)[0].question, "Вопрос номер 0?");
    }
}
