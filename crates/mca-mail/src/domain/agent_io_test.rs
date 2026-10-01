//! Tests for `agent_io.rs`.

#![cfg(test)]

use super::*;
use super::*;

fn q(text: &str) -> String {
    text.to_string()
}

#[test]
fn unanswered_filters_previously_asked_questions() {
    let result = QualificationResult {
        lead_id: None,
        requirements: vec![],
        request_summary: String::new(),
        questions: vec![
            q("Какой вес груза?"),
            q("В какой город доставка?"),
            q("Сколько стоит?"),
        ],
        scope: super::super::requirement::RequirementScope::Transport,
        confidence: Confidence::new(0.8),
        regulated_topics: vec![],
        contradictions: vec![],
    };
    let asked = vec![q("какой вес груза")];
    let left = result.unanswered(&asked);
    assert_eq!(left.len(), 2);
    assert!(left.iter().any(|x| x == "В какой город доставка?"));
}

#[test]
fn question_normalization_ignores_punctuation_and_case() {
    assert_eq!(normalize_question("Сколько  стоит?!"), "сколько стоит");
    assert_eq!(
        normalize_question("Какой вес груза"),
        normalize_question("  какой ВЕС, груза? ")
    );
}
