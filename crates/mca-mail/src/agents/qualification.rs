//! Lead Qualification Agent: extracts commercial parameters from client emails.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Agent, AgentIdentity};
use crate::domain::{AgentKind, EmailId, LeadId, RawRequirement};
use crate::error::AgentError;
use crate::orchestration::AgentContext;

/// What the qualification model produced.
///
/// String fields take any scalar: a model that answers `"company_name": true`
/// has still told us something, and failing the whole turn on a wrong type
/// would park an otherwise good extraction in human review.
#[derive(Debug, Serialize, Deserialize)]
pub struct QualificationOutput {
    pub lead_id: Option<LeadId>,
    pub first_email_id: Option<EmailId>,
    #[serde(default, deserialize_with = "crate::domain::flex::opt_string")]
    pub company_name: Option<String>,
    #[serde(default, deserialize_with = "crate::domain::flex::opt_string")]
    pub contact_name: Option<String>,
    #[serde(default, deserialize_with = "crate::domain::flex::opt_string")]
    pub contact_phone: Option<String>,
    #[serde(default, deserialize_with = "crate::domain::flex::string")]
    pub summary: String,
    #[serde(default, deserialize_with = "crate::domain::flex::string")]
    pub scope: String,
    #[serde(default)]
    pub needs_expert: bool,
    #[serde(default, deserialize_with = "crate::domain::flex::string_list")]
    pub questions: Vec<String>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default, deserialize_with = "crate::domain::flex::string_list")]
    pub regulated_topics: Vec<String>,
    /// Raw extraction. Every entry is re-validated by
    /// [`crate::domain::normalize`] before it is allowed near the database.
    #[serde(default)]
    pub requirements: Vec<RawRequirement>,
}

pub struct LeadQualificationAgent {
    llm: std::sync::Arc<dyn crate::llm::LlmProvider>,
}

impl LeadQualificationAgent {
    pub fn new(llm: std::sync::Arc<dyn crate::llm::LlmProvider>) -> Self {
        LeadQualificationAgent { llm }
    }

    /// `asked_before` is the list of questions already put to this customer.
    /// The model is told what it asked so it does not ask again; the caller
    /// still filters the result, because a model asked not to repeat itself
    /// will happily repeat itself.
    pub async fn qualify(
        &self,
        context: &AgentContext,
        asked_before: &[String],
    ) -> Result<QualificationOutput, AgentError> {
        let system = r#"You are a lead qualification agent for MCA Logistics, a company providing international logistics services.
Analyse the email and extract commercial details.

Extract what information is available:
- Company name, contact person, phone
- Type of request (transport, customs, procurement, full_import)
- Cargo details if mentioned (weight, volume, origin, destination)
- Whether the client needs expert advice

Every extracted value goes into "requirements" as an object:
{"field": "<field_name>", "value": "<verbatim value or null>", "state": "known|unknown|not_applicable|needs_confirmation", "unit": "<optional>", "confidence": 0.0-1.0, "evidence": "<exact span from the email that supports the value>"}

Use "known" ONLY when you can quote the exact words from the email in "evidence".
Use "needs_confirmation" when you inferred or paraphrased a value.
Use "not_applicable" when the field is irrelevant to this request.
Unknown field names are discarded, so use exactly the documented field names:
goods_name, goods_description, goods_quantity, goods_quantity_unit, goods_weight,
goods_weight_unit, goods_volume, goods_volume_unit, package_count,
origin_country, origin_city, destination_country, destination_city,
transport_mode, incoterms, desired_deadline, goods_value, goods_currency,
needs_insurance, needs_procurement, needs_foreign_supplier_payment,
needs_customs_clearance, needs_full_ved_support, deal_scheme_preference,
company_name, company_inn, contact_name, contact_position, contact_phone,
contact_telegram, contact_preferred_channel, additional_requirements.

Rules:
- Do NOT invent missing data.
- Distinguish stated facts from your inferences.
- Return questions for missing critical information. Never repeat a question
  that appears in the "already asked" list below.
- If the email asks for a phone call, set needs_expert=true.

Respond with JSON: {"lead_id": null, "first_email_id": null, "company_name": null, "contact_name": null, "contact_phone": null, "summary": "", "scope": "transport|customs|procurement|full_import", "needs_expert": false, "questions": [], "confidence": 0.0, "regulated_topics": [], "requirements": []}"#;

        let asked = if asked_before.is_empty() {
            "(none)".to_string()
        } else {
            asked_before
                .iter()
                .map(|q| format!("- {}", truncate(q, 300)))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let user = format!(
            "From: {}\nSubject: {}\n\nAlready asked this customer:\n{}\n\nBody:\n{}",
            context.email.from_address,
            context.email.subject,
            asked,
            truncate(&context.email.text_body, 4000)
        );

        let mut result: QualificationOutput =
            self.call_llm_structured(context, system, &user).await?;

        // lead_id and email_id always come from the server, never the model.
        result.first_email_id = Some(context.email_id);
        Ok(result)
    }
}

#[async_trait]
impl Agent for LeadQualificationAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::LeadQualification
    }
}

fn truncate(s: &str, max: usize) -> String {
    super::prompt::truncate_for_prompt(s, max)
}
