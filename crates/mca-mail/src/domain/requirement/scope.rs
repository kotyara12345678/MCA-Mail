//! Which service combination a lead is asking about.

use serde::{Deserialize, Serialize};

use super::super::ids::WireParseError;
use super::RequirementField;

/// Drives which requirements are collected and which become `not_applicable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementScope {
    /// Freight only: no procurement, no customs filing on the customer's behalf.
    Transport,
    /// Customs clearance for goods the customer already owns.
    Customs,
    /// Sourcing and payment for goods bought from a foreign supplier.
    Procurement,
    /// End-to-end: purchase, payment, freight, customs, delivery.
    FullImport,
}

impl RequirementScope {
    pub const ALL: &'static [RequirementScope] = &[
        RequirementScope::Transport,
        RequirementScope::Customs,
        RequirementScope::Procurement,
        RequirementScope::FullImport,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            RequirementScope::Transport => "transport",
            RequirementScope::Customs => "customs",
            RequirementScope::Procurement => "procurement",
            RequirementScope::FullImport => "full_import",
        }
    }

    /// Derive the scope from the classification the orchestrator already made,
    /// so the qualification agent does not have to re-decide the service type.
    pub fn from_category(category: super::super::class::EmailCategory) -> Self {
        use super::super::class::EmailCategory as C;
        match category {
            C::TransportRequest => RequirementScope::Transport,
            C::CustomsRequest => RequirementScope::Customs,
            C::ProcurementRequest => RequirementScope::Procurement,
            _ => RequirementScope::FullImport,
        }
    }
}

impl std::str::FromStr for RequirementScope {
    type Err = super::super::ids::WireParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let needle = s.trim().to_ascii_lowercase();
        RequirementScope::ALL
            .iter()
            .find(|candidate| candidate.as_str() == needle)
            .copied()
            .ok_or(WireParseError {
                type_name: "RequirementScope",
                value: needle,
            })
    }
}

/// `FromStr` for the field enum, sharing the `ALL` + `as_str` source of truth
/// with [`wire_enum!`] so both enums parse identically.
impl std::str::FromStr for RequirementField {
    type Err = super::super::ids::WireParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let needle = s.trim().to_ascii_lowercase();
        RequirementField::ALL
            .iter()
            .find(|candidate| candidate.as_str() == needle)
            .copied()
            .ok_or(WireParseError {
                type_name: "RequirementField",
                value: needle,
            })
    }
}
