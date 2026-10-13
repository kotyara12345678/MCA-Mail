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

// ---------------------------------------------------------------------------
// the red flag rule
// ---------------------------------------------------------------------------

const SURE: f32 = 0.9;
const UNSURE: f32 = 0.5;

#[test]
fn confident_demand_is_an_order() {
    assert!(is_order_related(
        EmailCategory::TransportRequest,
        Some(SpamVerdict::NotSpam),
        SURE,
        false
    ));
    assert!(is_order_related(EmailCategory::NewLead, None, SURE, false));
    assert!(is_order_related(
        EmailCategory::ProcurementRequest,
        Some(SpamVerdict::NotSpam),
        SURE,
        false
    ));
}

#[test]
fn uncertain_demand_without_context_is_not_flagged() {
    // The model guessed the category but was not sure, and the thread says
    // nothing: an ambiguous message must stay unflagged.
    assert!(!is_order_related(
        EmailCategory::TransportRequest,
        Some(SpamVerdict::NotSpam),
        UNSURE,
        false
    ));
    assert!(!is_order_related(
        EmailCategory::Uncertain,
        None,
        SURE,
        false
    ));
    assert!(!is_order_related(
        EmailCategory::Other,
        Some(SpamVerdict::NotSpam),
        SURE,
        false
    ));
}

#[test]
fn uncertain_demand_in_an_order_thread_is_flagged() {
    // A follow-up the model reads as demand but scores low still belongs to
    // the order the thread already carries.
    assert!(is_order_related(
        EmailCategory::ExistingClient,
        Some(SpamVerdict::NotSpam),
        UNSURE,
        true
    ));
}

#[test]
fn neutral_category_in_an_order_thread_is_flagged() {
    // An invoice, a document, a confirmation: neutral alone, order traffic
    // inside a thread that already is one.
    assert!(is_order_related(
        EmailCategory::DocumentRequest,
        Some(SpamVerdict::NotSpam),
        SURE,
        true
    ));
    assert!(is_order_related(
        EmailCategory::DocumentRequest,
        Some(SpamVerdict::NotSpam),
        UNSURE,
        true
    ));
}

#[test]
fn neutral_category_without_context_is_not_flagged() {
    // A document from a stranger is not an order.
    assert!(!is_order_related(
        EmailCategory::DocumentRequest,
        Some(SpamVerdict::NotSpam),
        SURE,
        false
    ));
    assert!(!is_order_related(EmailCategory::Partner, None, SURE, false));
}

#[test]
fn spam_and_machine_mail_never_earn_the_flag() {
    // Whatever the category came out as, spam, ads and automated
    // notifications (HH.ru, portal digests) are not order traffic.
    for spam in [
        SpamVerdict::Spam,
        SpamVerdict::Advertisement,
        SpamVerdict::AutomatedNotification,
    ] {
        for category in [
            EmailCategory::TransportRequest,
            EmailCategory::NewLead,
            EmailCategory::DocumentRequest,
        ] {
            for context in [false, true] {
                assert!(
                    !is_order_related(category, Some(spam), SURE, context),
                    "{category:?} + {spam:?} + context={context} must stay unflagged"
                );
            }
        }
    }
}

#[test]
fn demand_never_earns_the_flag_from_a_low_confidence_alone() {
    // Confidence below the auto-action threshold with no thread proof is
    // exactly the ambiguity the flag must not guess through.
    assert!(!is_order_related(
        EmailCategory::FullImportRequest,
        Some(SpamVerdict::Uncertain),
        Confidence::AUTO_ACTION_THRESHOLD - 0.01,
        false
    ));
    assert!(is_order_related(
        EmailCategory::FullImportRequest,
        Some(SpamVerdict::Uncertain),
        Confidence::AUTO_ACTION_THRESHOLD,
        false
    ));
}
