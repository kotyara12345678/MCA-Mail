//! The closed set of commercial parameters MCA needs to quote a shipment.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementField {
    // --- goods -----------------------------------------------------------
    GoodsName,
    GoodsDescription,
    GoodsQuantity,
    GoodsQuantityUnit,
    GoodsWeight,
    GoodsWeightUnit,
    GoodsVolume,
    GoodsVolumeUnit,
    PackageCount,
    // --- route -----------------------------------------------------------
    OriginCountry,
    OriginCity,
    DestinationCountry,
    DestinationCity,
    TransportMode,
    Incoterms,
    // --- timing / money --------------------------------------------------
    DesiredDeadline,
    GoodsValue,
    GoodsCurrency,
    // --- requested services ---------------------------------------------
    NeedsInsurance,
    NeedsProcurement,
    NeedsForeignSupplierPayment,
    NeedsCustomsClearance,
    NeedsFullVedSupport,
    DealSchemePreference,
    // --- customer identity ------------------------------------------------
    CompanyName,
    CompanyInn,
    ContactName,
    ContactPosition,
    ContactPhone,
    ContactTelegram,
    ContactPreferredChannel,
    // --- meta -------------------------------------------------------------
    AdditionalRequirements,
}

impl RequirementField {
    pub const ALL: &'static [RequirementField] = &[
        RequirementField::GoodsName,
        RequirementField::GoodsDescription,
        RequirementField::GoodsQuantity,
        RequirementField::GoodsQuantityUnit,
        RequirementField::GoodsWeight,
        RequirementField::GoodsWeightUnit,
        RequirementField::GoodsVolume,
        RequirementField::GoodsVolumeUnit,
        RequirementField::PackageCount,
        RequirementField::OriginCountry,
        RequirementField::OriginCity,
        RequirementField::DestinationCountry,
        RequirementField::DestinationCity,
        RequirementField::TransportMode,
        RequirementField::Incoterms,
        RequirementField::DesiredDeadline,
        RequirementField::GoodsValue,
        RequirementField::GoodsCurrency,
        RequirementField::NeedsInsurance,
        RequirementField::NeedsProcurement,
        RequirementField::NeedsForeignSupplierPayment,
        RequirementField::NeedsCustomsClearance,
        RequirementField::NeedsFullVedSupport,
        RequirementField::DealSchemePreference,
        RequirementField::CompanyName,
        RequirementField::CompanyInn,
        RequirementField::ContactName,
        RequirementField::ContactPosition,
        RequirementField::ContactPhone,
        RequirementField::ContactTelegram,
        RequirementField::ContactPreferredChannel,
        RequirementField::AdditionalRequirements,
    ];

    /// Stable snake_case name, also the database column name.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::GoodsName => "goods_name",
            Self::GoodsDescription => "goods_description",
            Self::GoodsQuantity => "goods_quantity",
            Self::GoodsQuantityUnit => "goods_quantity_unit",
            Self::GoodsWeight => "goods_weight",
            Self::GoodsWeightUnit => "goods_weight_unit",
            Self::GoodsVolume => "goods_volume",
            Self::GoodsVolumeUnit => "goods_volume_unit",
            Self::PackageCount => "package_count",
            Self::OriginCountry => "origin_country",
            Self::OriginCity => "origin_city",
            Self::DestinationCountry => "destination_country",
            Self::DestinationCity => "destination_city",
            Self::TransportMode => "transport_mode",
            Self::Incoterms => "incoterms",
            Self::DesiredDeadline => "desired_deadline",
            Self::GoodsValue => "goods_value",
            Self::GoodsCurrency => "goods_currency",
            Self::NeedsInsurance => "needs_insurance",
            Self::NeedsProcurement => "needs_procurement",
            Self::NeedsForeignSupplierPayment => "needs_foreign_supplier_payment",
            Self::NeedsCustomsClearance => "needs_customs_clearance",
            Self::NeedsFullVedSupport => "needs_full_ved_support",
            Self::DealSchemePreference => "deal_scheme_preference",
            Self::CompanyName => "company_name",
            Self::CompanyInn => "company_inn",
            Self::ContactName => "contact_name",
            Self::ContactPosition => "contact_position",
            Self::ContactPhone => "contact_phone",
            Self::ContactTelegram => "contact_telegram",
            Self::ContactPreferredChannel => "contact_preferred_channel",
            Self::AdditionalRequirements => "additional_requirements",
        }
    }
}
