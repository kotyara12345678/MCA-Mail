use super::*;
use crate::domain::class::FieldState;
use uuid::Uuid;

#[test]
fn transport_scope_drops_procurement_only_fields() {
    let scope = RequirementScope::Transport;
    assert!(!RequirementField::GoodsValue.applies_to(scope));
    assert!(RequirementField::OriginCountry.applies_to(scope));
    assert!(!RequirementField::NeedsProcurement.applies_to(scope));
}

#[test]
fn full_import_scope_keeps_everything() {
    let scope = RequirementScope::FullImport;
    for field in RequirementField::ALL {
        assert!(field.applies_to(scope), "{} should apply", field.as_str());
    }
}

#[test]
fn identity_and_quote_blocking_classification() {
    assert!(RequirementField::CompanyInn.is_identity());
    assert!(!RequirementField::CompanyInn.is_quote_blocking());
    assert!(RequirementField::GoodsWeight.is_quote_blocking());
    assert!(RequirementField::NeedsInsurance.is_flag());
}

#[test]
fn empty_requirement_has_unknown_state() {
    let r = LeadRequirement::empty(Uuid::new_v4(), RequirementField::GoodsName);
    assert_eq!(r.state, FieldState::Unknown);
    assert!(r.value.is_none());
    let i = LeadRequirement::inferred(Uuid::new_v4(), RequirementField::GoodsName, "станок");
    assert_eq!(i.state, FieldState::NeedsConfirmation);
    assert!(!i.state.is_trusted());
}

#[test]
fn field_names_round_trip_through_from_str() {
    for field in RequirementField::ALL {
        assert_eq!(field.as_str().parse::<RequirementField>().unwrap(), *field);
    }
    assert!("no_such_field".parse::<RequirementField>().is_err());
}
