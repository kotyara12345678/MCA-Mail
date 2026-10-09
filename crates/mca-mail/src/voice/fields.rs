//! Field labels and value validation for voice-spoken requirements.
//!
//! Labels exist so the call agent asks the customer a plain-language question
//! and reports back the same wording; validation exists so junk speech cannot
//! persist. Numeric checks follow the email pipeline's rule that a quantity,
//! weight, volume or value on the wire must parse as a non-negative number.

use crate::domain::RequirementField;
use crate::error::AppError;

/// Extremes mirrored from `domain/requirement/normalize.rs` so a value that
/// would be rejected for email mail is equally rejected on the voice path.
pub const MAX_VALUE_LEN: usize = 500;
pub const MAX_UNIT_LEN: usize = 32;

/// Human-readable Russian label for a requirement field, so the voice agent
/// phrases its question naturally without inventing product vocabulary.
pub fn label(field: RequirementField) -> &'static str {
    use RequirementField as F;
    match field {
        F::GoodsName => "наименование груза",
        F::GoodsDescription => "описание груза",
        F::GoodsQuantity => "количество груза",
        F::GoodsQuantityUnit => "единица измерения груза",
        F::GoodsWeight => "вес груза",
        F::GoodsWeightUnit => "единица веса",
        F::GoodsVolume => "объём груза",
        F::GoodsVolumeUnit => "единица объёма",
        F::PackageCount => "количество мест",
        F::OriginCountry => "страна отправления",
        F::OriginCity => "город отправления",
        F::DestinationCountry => "страна назначения",
        F::DestinationCity => "город назначения",
        F::TransportMode => "способ перевозки",
        F::Incoterms => "условия поставки (Incoterms)",
        F::DesiredDeadline => "желаемый срок",
        F::GoodsValue => "стоимость груза",
        F::GoodsCurrency => "валюта",
        F::NeedsInsurance => "нужна ли страховка",
        F::NeedsProcurement => "требуется ли закупка",
        F::NeedsForeignSupplierPayment => "нужен ли платёж иностранному поставщику",
        F::NeedsCustomsClearance => "нужно ли таможенное оформление",
        F::NeedsFullVedSupport => "нужна ли полная ВЭД-поддержка",
        F::DealSchemePreference => "предпочтительная схема сделки",
        F::CompanyName => "название компании",
        F::CompanyInn => "ИНН компании",
        F::ContactName => "имя контактного лица",
        F::ContactPosition => "должность контактного лица",
        F::ContactPhone => "телефон контактного лица",
        F::ContactTelegram => "Telegram контактного лица",
        F::ContactPreferredChannel => "предпочтительный канал связи",
        F::AdditionalRequirements => "дополнительные требования",
    }
}

/// Fields expected to hold a non-negative number.
fn numeric(field: RequirementField) -> bool {
    use RequirementField as F;
    matches!(
        field,
        F::GoodsQuantity | F::GoodsWeight | F::GoodsVolume | F::GoodsValue | F::PackageCount
    )
}

/// Trim, enforce length caps, and reject negative / non-numeric values where
/// the field must be numeric. Returns the cleaned value.
pub fn sanitize(field: RequirementField, value: &str) -> Result<String, AppError> {
    let cleaned = value.trim();
    if cleaned.is_empty() {
        return Err(AppError::invalid(format!(
            "{} не может быть пустым",
            label(field)
        )));
    }
    if cleaned.len() > MAX_VALUE_LEN {
        return Err(AppError::invalid(format!(
            "{} длиннее {MAX_VALUE_LEN} символов",
            label(field)
        )));
    }
    if numeric(field) {
        let n: f64 = cleaned
            .replace(',', ".")
            .parse()
            .map_err(|_| AppError::invalid(format!("{} должно быть числом", label(field))))?;
        if n < 0.0 {
            return Err(AppError::invalid(format!(
                "{} не может быть отрицательным",
                label(field)
            )));
        }
    }
    Ok(cleaned.to_string())
}

/// Validate a unit string if one was given.
pub fn sanitize_unit(unit: Option<&str>) -> Result<Option<String>, AppError> {
    unit.map(str::trim)
        .filter(|u| !u.is_empty())
        .map(|u| {
            if u.len() > MAX_UNIT_LEN {
                Err(AppError::invalid("единица измерения слишком длинная"))
            } else {
                Ok(u.to_string())
            }
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_russian_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for field in RequirementField::ALL {
            let l = label(*field);
            assert!(!l.is_empty());
            assert!(seen.insert(l), "duplicate label for {field:?}");
        }
    }

    #[test]
    fn negative_weight_rejected() {
        assert!(sanitize(RequirementField::GoodsWeight, "-10").is_err());
    }

    #[test]
    fn decimal_weight_ok() {
        assert_eq!(
            sanitize(RequirementField::GoodsWeight, "12,5").unwrap(),
            "12,5"
        );
    }

    #[test]
    fn text_field_accepts_anything() {
        assert_eq!(
            sanitize(RequirementField::GoodsName, "  запчасти  ").unwrap(),
            "запчасти"
        );
    }

    #[test]
    fn empty_rejected() {
        assert!(sanitize(RequirementField::GoodsName, "   ").is_err());
    }

    #[test]
    fn overlong_rejected() {
        let long = "а".repeat(501);
        assert!(sanitize(RequirementField::AdditionalRequirements, &long).is_err());
    }
}
