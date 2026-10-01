//! Per-field classification rules.
//!
//! Kept apart from the enum definition so the answer to "what may be asked and
//! when" is reviewable on its own: it is the part that decides whether a
//! customer gets asked about a purchase price when they only wanted freight.

use super::{RequirementField, RequirementScope};

impl RequirementField {
    /// Whether a boolean-flagged field is expected to hold yes/no.
    pub const fn is_flag(&self) -> bool {
        matches!(
            self,
            RequirementField::NeedsInsurance
                | RequirementField::NeedsProcurement
                | RequirementField::NeedsForeignSupplierPayment
                | RequirementField::NeedsCustomsClearance
                | RequirementField::NeedsFullVedSupport
        )
    }

    /// Fields the agent must never invent a value for.
    pub const fn is_identity(&self) -> bool {
        matches!(
            self,
            RequirementField::CompanyName
                | RequirementField::CompanyInn
                | RequirementField::ContactName
                | RequirementField::ContactPhone
        )
    }

    /// Fields that, when missing, prevent MCA from producing a quote.
    pub const fn is_quote_blocking(&self) -> bool {
        matches!(
            self,
            RequirementField::GoodsName
                | RequirementField::OriginCountry
                | RequirementField::DestinationCountry
                | RequirementField::GoodsWeight
                | RequirementField::GoodsQuantity
        )
    }

    /// Does the field apply to the requested service combination at all?
    ///
    /// Fields that do not apply move to `not_applicable` instead of being asked
    /// about forever. Flag fields are answered yes/no rather than filled with a
    /// value, but they are still relevant to a scope, so they are kept here.
    pub fn applies_to(&self, scope: RequirementScope) -> bool {
        match scope {
            RequirementScope::Transport => self.in_transport_scope(),
            RequirementScope::Customs => self.in_customs_scope(),
            RequirementScope::Procurement | RequirementScope::FullImport => true,
        }
    }

    fn in_transport_scope(&self) -> bool {
        use RequirementField as F;
        matches!(
            self,
            F::OriginCountry
                | F::OriginCity
                | F::DestinationCountry
                | F::DestinationCity
                | F::TransportMode
                | F::Incoterms
                | F::PackageCount
                | F::NeedsInsurance
                | F::GoodsName
                | F::GoodsDescription
                | F::GoodsQuantity
                | F::GoodsQuantityUnit
                | F::GoodsWeight
                | F::GoodsWeightUnit
                | F::GoodsVolume
                | F::GoodsVolumeUnit
                | F::DesiredDeadline
                | F::CompanyName
                | F::CompanyInn
                | F::ContactName
                | F::ContactPosition
                | F::ContactPhone
                | F::ContactTelegram
                | F::ContactPreferredChannel
                | F::AdditionalRequirements
        )
    }

    fn in_customs_scope(&self) -> bool {
        use RequirementField as F;
        matches!(
            self,
            F::OriginCountry
                | F::OriginCity
                | F::DestinationCountry
                | F::DestinationCity
                | F::TransportMode
                | F::GoodsName
                | F::GoodsDescription
                | F::PackageCount
                | F::GoodsValue
                | F::GoodsCurrency
                | F::CompanyName
                | F::CompanyInn
                | F::ContactName
                | F::ContactPosition
                | F::ContactPhone
                | F::ContactTelegram
                | F::ContactPreferredChannel
                | F::AdditionalRequirements
        )
    }
}
