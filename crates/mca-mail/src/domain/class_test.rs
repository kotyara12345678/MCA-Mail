//! Tests for `class.rs`.

#![cfg(test)]

use super::*;
use super::*;

#[test]
fn category_routing_rules() {
    assert!(EmailCategory::FullImportRequest.is_demand());
    assert!(!EmailCategory::DocumentRequest.is_demand());
    assert!(EmailCategory::Complaint.requires_human());
    assert!(!EmailCategory::Spam.allows_reply());
    assert!(EmailCategory::NewLead.allows_reply());
}

#[test]
fn spam_verdict_routing_rules() {
    assert!(SpamVerdict::Spam.quarantine());
    assert!(!SpamVerdict::PhishingSuspected.quarantine());
    assert!(SpamVerdict::PhishingSuspected.needs_human_review());
    assert!(SpamVerdict::Uncertain.needs_human_review());
    assert!(!SpamVerdict::NotSpam.needs_human_review());
}

#[test]
fn confidence_is_clamped() {
    assert_eq!(Confidence::new(1.7).value(), 1.0);
    assert_eq!(Confidence::new(-2.0).value(), 0.0);
    assert!(!Confidence::new(0.4).allows_auto_action());
    assert!(Confidence::new(0.9).allows_auto_action());
}

#[test]
fn wire_round_trip() {
    assert_eq!(
        "full_import_request".parse::<EmailCategory>().unwrap(),
        EmailCategory::FullImportRequest
    );
    assert!("nope".parse::<EmailCategory>().is_err());
    assert_eq!(SpamVerdict::NotSpam.to_string(), "not_spam");
}
