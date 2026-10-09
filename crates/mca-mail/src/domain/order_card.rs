//! The customer-facing order card: what the caller receives after the phone
//! call, built only from what is already stored.
//!
//! Rendering is a pure function of the database state, so it is testable
//! without a mailbox. Two rules shape this card: nothing is invented (an
//! absent field prints `не предоставлено`), and nothing internal leaks — no
//! dialogue transcript, no confidence score, no manager-facing wording. The
//! customer sees their own request back, confirmed, plus the one thing they
//! can act on: what is still missing for a quote.

use crate::domain::{Lead, LeadRequirement, LeadStatus};

use super::manager_card::{
    escape, gap_label, quantity_of, route_of, scope_label, value_of, UNKNOWN,
};

/// Everything the card is built from. Assembled by the caller, rendered here.
#[derive(Debug, Clone)]
pub struct OrderCard<'a> {
    pub lead: &'a Lead,
    pub requirements: &'a [LeadRequirement],
    pub generated_at: chrono::DateTime<chrono::Utc>,
}

/// Status wording for the customer: business state, not CRM internals —
/// "готов менеджеру" or a confidence number would only confuse the recipient.
fn customer_status_label(status: LeadStatus) -> &'static str {
    match status {
        LeadStatus::New => "принято",
        LeadStatus::Processing => "в работе",
        LeadStatus::AwaitingCustomer => "ждём уточнения от вас",
        LeadStatus::Qualified => "данные собраны, готовим расчёт",
        LeadStatus::NeedsHumanReview => "передано менеджеру",
        LeadStatus::HandedOff => "передано менеджеру",
        LeadStatus::InProgress => "менеджер в работе",
        LeadStatus::Won => "сделка подтверждена",
        LeadStatus::Lost => "заявка закрыта",
        LeadStatus::Closed => "заявка закрыта",
    }
}

impl OrderCard<'_> {
    fn need(&self, field: crate::domain::RequirementField) -> String {
        value_of(self.requirements, field).unwrap_or_else(|| UNKNOWN.to_string())
    }

    /// Subject line: the goods when they are known, the company below that,
    /// otherwise an honest generic — a subject that guesses is a bad ad.
    pub fn subject(&self) -> String {
        let title = value_of(
            self.requirements,
            crate::domain::RequirementField::GoodsName,
        )
        .or_else(|| self.lead.company_name.clone())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "заявка".to_string());
        format!("Карточка заказа: {title} — MCA Logistics")
    }

    /// What still blocks a quote, in the customer's words: they are the one
    /// who has to supply it.
    fn gaps(&self) -> String {
        let missing = Lead::blocking_gaps(self.requirements);
        if missing.is_empty() {
            return "ничего — полный комплект".to_string();
        }
        missing
            .iter()
            .map(|f| gap_label(*f))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn summary(&self) -> String {
        self.lead
            .summary
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(UNKNOWN)
            .to_string()
    }

    fn rows(&self) -> Vec<(&'static str, String)> {
        use crate::domain::RequirementField as F;
        vec![
            ("Номер обращения", self.lead.id.to_string()),
            (
                "Статус",
                customer_status_label(self.lead.status).to_string(),
            ),
            ("Услуги", scope_label(self.lead.scope).to_string()),
            ("Компания", self.need(F::CompanyName)),
            ("Контакт", self.need(F::ContactName)),
            ("Телефон", self.need(F::ContactPhone)),
            ("Email", self.lead.contact_email.trim().to_ascii_lowercase()),
            ("Груз", self.need(F::GoodsName)),
            ("Вес", quantity_of(self.requirements, F::GoodsWeight)),
            ("Объём", quantity_of(self.requirements, F::GoodsVolume)),
            (
                "Количество",
                quantity_of(self.requirements, F::GoodsQuantity),
            ),
            ("Маршрут", route_of(self.requirements)),
            ("Сроки", self.need(F::DesiredDeadline)),
            ("Транспорт", self.need(F::TransportMode)),
            ("Инкотермс", self.need(F::Incoterms)),
            (
                "Дополнительные условия",
                self.need(F::AdditionalRequirements),
            ),
            ("Чего не хватает для расчёта", self.gaps()),
        ]
    }

    /// Plain-text card: what an email client without HTML shows.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("КАРТОЧКА ЗАКАЗА — MCA Logistics\n");
        out.push_str(&"=".repeat(48));
        out.push('\n');
        for (label, value) in self.rows() {
            out.push_str(&format!("{label:<28} {value}\n"));
        }
        out.push_str(&format!("\nРезюме:\n{}\n", self.summary()));
        out.push_str(&format!(
            "\nСформировано: {}\n",
            self.generated_at.to_rfc3339()
        ));
        out
    }

    /// HTML card: the same rows, escaped, for a client that renders mail.
    pub fn to_html(&self) -> String {
        let mut rows = String::new();
        for (label, value) in self.rows() {
            rows.push_str(&format!(
                "<tr><th>{}</th><td>{}</td></tr>\n",
                escape(label),
                escape(&value)
            ));
        }
        format!(
            "<!doctype html><html><body style=\"font-family:Arial,sans-serif;color:#1a1a1a\">\
             <h2 style=\"margin-bottom:4px\">Карточка заказа — MCA Logistics</h2>\
             <table style=\"border-collapse:collapse;width:100%;max-width:720px\">{rows}</table>\
             <h3>Резюме</h3><p style=\"white-space:pre-wrap\">{summary}</p>\
             <p style=\"color:#777;font-size:12px\">Сформировано: {at}</p>\
             </body></html>",
            rows = rows,
            summary = escape(&self.summary()),
            at = escape(&self.generated_at.to_rfc3339()),
        )
    }
}

#[cfg(test)]
#[path = "order_card_tests.rs"]
mod tests;
