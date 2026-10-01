//! Deterministic mocks and fixtures for MCA Mail tests.
//!
//! Everything here is safe to use offline: no network calls, no real
//! credentials, no external state. Test outputs are stable strings that do not
//! depend on a live model, so assertions never flake.

use std::collections::HashMap;
use std::sync::Mutex;

/// Deterministic LLM responses keyed by a message-content prefix.
///
/// A test registers canned outputs with [`StubLlm::stub`], then feeds them
/// through the same interface the agents use. The last registered stub wins,
/// and an unmatched call records a miss so a test can assert no request went
/// unanswered.
#[derive(Default)]
pub struct StubLlm {
    responses: Mutex<HashMap<String, String>>,
    calls: Mutex<Vec<String>>,
    misses: Mutex<Vec<String>>,
}

impl StubLlm {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a canned reply returned whenever the user message contains
    /// `prefix`. Use `"*"` as a catch-all default.
    pub fn stub(&self, prefix: &str, reply: &str) {
        self.responses
            .lock()
            .unwrap()
            .insert(prefix.to_string(), reply.to_string());
    }

    /// Register a canned JSON reply.
    pub fn stub_json(&self, prefix: &str, value: &impl serde::Serialize) {
        let json = serde_json::to_string(value).unwrap_or_default();
        self.stub(prefix, &json);
    }

    /// Resolve a reply for `content`, recording the request.
    pub fn respond(&self, content: &str) -> Option<String> {
        self.calls.lock().unwrap().push(content.to_string());
        let responses = self.responses.lock().unwrap();
        for (prefix, reply) in responses.iter() {
            if prefix == "*" || content.contains(prefix) {
                return Some(reply.clone());
            }
        }
        self.misses.lock().unwrap().push(content.to_string());
        None
    }

    /// All user messages sent to the stub, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    /// Messages that matched no stub — a test should assert this is empty.
    pub fn misses(&self) -> Vec<String> {
        self.misses.lock().unwrap().clone()
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    pub fn clear(&self) {
        self.responses.lock().unwrap().clear();
        self.calls.lock().unwrap().clear();
        self.misses.lock().unwrap().clear();
    }
}

/// Canned structured replies for the standard agent outputs.
pub mod replies {
    use serde_json::json;

    pub fn not_spam() -> serde_json::Value {
        json!({
            "verdict": "not_spam",
            "confidence": 0.95,
            "explanation": "Commercial logistics request",
            "markers": []
        })
    }

    pub fn spam() -> serde_json::Value {
        json!({
            "verdict": "spam",
            "confidence": 0.98,
            "explanation": "Unsolicited bulk advertising",
            "markers": ["bulk_ad"]
        })
    }

    pub fn classification_lead() -> serde_json::Value {
        json!({
            "category": "new_lead",
            "confidence": 0.9,
            "explanation": "New commercial enquiry",
            "requires_human": false,
            "suggested_action": "qualify"
        })
    }

    pub fn qualification_transport() -> serde_json::Value {
        json!({
            "lead_id": null,
            "first_email_id": null,
            "company_name": "ООО ОптОрг",
            "contact_name": "Иван Петров",
            "contact_phone": "+7 (495) 123-45-67",
            "summary": "Перевозка оборудования из Китая в Россию",
            "scope": "transport",
            "needs_expert": false,
            "questions": [],
            "confidence": 0.85,
            "regulated_topics": []
        })
    }

    pub fn communication_draft() -> serde_json::Value {
        json!({
            "subject": "Re: Доставка оборудования из Китая",
            "body": "Здравствуйте, Иван! Спасибо за запрос...",
            "disposition": "draft",
            "handoff_requested": false,
            "handoff_reason": null,
            "confidence": 0.9,
            "rationale": "Уточняющие вопросы"
        })
    }

    pub fn communication_handoff() -> serde_json::Value {
        json!({
            "subject": "Re: Доставка оборудования из Китая",
            "body": "Здравствуйте! Передам ваш запрос менеджеру.",
            "disposition": "draft",
            "handoff_requested": true,
            "handoff_reason": "callback_requested",
            "confidence": 0.9,
            "rationale": "Клиент просит звонок"
        })
    }

    pub fn handoff_package() -> serde_json::Value {
        json!({
            "reason": "callback_requested",
            "priority": 30,
            "cargo_summary": "2 станка ЧПУ, 1800 кг каждый, 8 м3",
            "route_summary": "Шэньчжэнь → Москва",
            "requested_service": "transport",
            "missing_information": ["точные сроки"],
            "open_questions": [],
            "conversation_digest": "Клиент запросил стоимость морской перевозки",
            "checks_performed": [],
            "unresolved_topics": [],
            "original_request": "Доставка оборудования из Китая"
        })
    }
}
