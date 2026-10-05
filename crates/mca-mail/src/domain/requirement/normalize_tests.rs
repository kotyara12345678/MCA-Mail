//! Tests for the server-side requirement normaliser.

#![cfg(test)]

use super::*;

fn raw(field: &str) -> RawRequirement {
    RawRequirement {
        field: field.to_string(),
        ..Default::default()
    }
}

#[test]
fn an_unrecognised_field_is_dropped_not_guessed() {
    let (out, report) = normalize(uuid::Uuid::new_v4(), &[raw("colour_of_the_cat")]);
    assert!(out.is_empty());
    assert_eq!(report.unknown_fields, vec!["colour_of_the_cat".to_string()]);
    assert!(!report.is_clean());
}

#[test]
fn an_empty_value_becomes_unknown_not_empty_string() {
    let item = RawRequirement {
        value: Some("   ".into()),
        ..raw("goods_name")
    };
    let (out, report) = normalize(uuid::Uuid::new_v4(), &[item]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].state, FieldState::Unknown);
    assert_eq!(out[0].value, None);
    assert_eq!(out[0].confidence, None);
    assert!(report.is_clean());
}

#[test]
fn known_without_evidence_is_downgraded_to_a_guess() {
    let item = RawRequirement {
        value: Some("станок".into()),
        state: Some("known".into()),
        ..raw("goods_name")
    };
    let (out, _) = normalize(uuid::Uuid::new_v4(), &[item]);
    assert_eq!(out[0].state, FieldState::NeedsConfirmation);
    assert_eq!(out[0].source, RequirementSource::AiInference);
    assert_eq!(out[0].evidence, None);
}

#[test]
fn known_with_evidence_is_recorded_as_a_customer_fact() {
    let item = RawRequirement {
        value: Some("станок".into()),
        state: Some("known".into()),
        evidence: Some("доставить станок из Китая".into()),
        ..raw("goods_name")
    };
    let (out, _) = normalize(uuid::Uuid::new_v4(), &[item]);
    assert_eq!(out[0].state, FieldState::Known);
    assert_eq!(out[0].source, RequirementSource::Customer);
    assert!(out[0].evidence.is_some());
}

#[test]
fn an_invalid_state_falls_back_instead_of_failing_the_batch() {
    let item = RawRequirement {
        value: Some("Китай".into()),
        state: Some("определено".into()),
        ..raw("origin_country")
    };
    let (out, report) = normalize(uuid::Uuid::new_v4(), &[item]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].state, FieldState::NeedsConfirmation);
    assert_eq!(report.invalid_states, vec!["определено".to_string()]);
}

#[test]
fn duplicate_fields_collapse_onto_the_strongest_answer() {
    let weak = RawRequirement {
        value: Some("Китай".into()),
        ..raw("origin_country")
    };
    let strong = RawRequirement {
        value: Some("Китай".into()),
        state: Some("known".into()),
        evidence: Some("из Китая".into()),
        ..raw("origin_country")
    };
    let (out, report) = normalize(uuid::Uuid::new_v4(), &[weak.clone(), strong.clone()]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].state, FieldState::Known);
    assert_eq!(report.merged_fields, vec!["origin_country".to_string()]);

    // Order must not matter: the weaker answer must not overwrite the stronger.
    let (out, _) = normalize(uuid::Uuid::new_v4(), &[strong, weak]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].state, FieldState::Known);
}

#[test]
fn not_applicable_survives_with_no_value() {
    let item = RawRequirement {
        state: Some("not_applicable".into()),
        ..raw("needs_procurement")
    };
    let (out, _) = normalize(uuid::Uuid::new_v4(), &[item]);
    assert_eq!(out[0].state, FieldState::NotApplicable);
    assert_eq!(out[0].source, RequirementSource::AiInference);
}

#[test]
fn oversized_values_are_bounded_not_rejected() {
    let item = RawRequirement {
        value: Some("x".repeat(5_000)),
        ..raw("additional_requirements")
    };
    let (out, _) = normalize(uuid::Uuid::new_v4(), &[item]);
    assert!(out[0].value.as_ref().unwrap().chars().count() <= 501);
}

#[test]
fn confidence_is_clamped_and_dropped_when_there_is_nothing_to_be_confident_about() {
    let confident = RawRequirement {
        value: Some("Китай".into()),
        confidence: Some(4.2),
        ..raw("origin_country")
    };
    let (out, _) = normalize(uuid::Uuid::new_v4(), &[confident]);
    assert_eq!(out[0].confidence, Some(1.0));

    let nothing = RawRequirement {
        confidence: Some(0.9),
        ..raw("origin_country")
    };
    let (out, _) = normalize(uuid::Uuid::new_v4(), &[nothing]);
    assert_eq!(out[0].confidence, None);
}
