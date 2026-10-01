//! One extracted commercial parameter and its provenance.

use serde::{Deserialize, Serialize};

use crate::domain::class::FieldState;
use crate::domain::ids::LeadId;
use crate::domain::wire_enum;

use super::RequirementField;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeadRequirement {
    pub lead_id: LeadId,
    pub field: RequirementField,
    pub value: Option<String>,
    pub state: FieldState,
    /// `customer` when the customer stated it, `ai_inference` when the model
    /// inferred it, `manager` when a human entered it.
    pub source: RequirementSource,
    /// Free-form unit, e.g. `кг`, `м3`, `шт`.
    pub unit: Option<String>,
    /// `null` for `not_applicable` and `unknown` entries.
    pub confidence: Option<f32>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

wire_enum! {
    RequirementSource {
        Customer => "customer",
        AiInference => "ai_inference",
        Manager => "manager",
    }
}

impl LeadRequirement {
    pub fn empty(lead_id: LeadId, field: RequirementField) -> Self {
        Self {
            lead_id,
            field,
            value: None,
            state: FieldState::Unknown,
            source: RequirementSource::AiInference,
            unit: None,
            confidence: None,
            updated_at: chrono::Utc::now(),
        }
    }

    /// A value the model produced. Never treated as confirmed on its own.
    pub fn inferred(lead_id: LeadId, field: RequirementField, value: impl Into<String>) -> Self {
        Self {
            value: Some(value.into()),
            state: FieldState::NeedsConfirmation,
            source: RequirementSource::AiInference,
            confidence: Some(0.5),
            ..Self::empty(lead_id, field)
        }
    }

    /// A value the customer stated explicitly.
    pub fn stated(lead_id: LeadId, field: RequirementField, value: impl Into<String>) -> Self {
        Self {
            value: Some(value.into()),
            state: FieldState::Known,
            source: RequirementSource::Customer,
            confidence: Some(1.0),
            ..Self::empty(lead_id, field)
        }
    }
}
