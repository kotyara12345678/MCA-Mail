//! The manager card: one self-contained summary of a qualified lead.
//!
//! Rendering is a pure function of what is in the database, so it can be
//! tested without a mailbox or a model. Nothing here invents a value — an
//! absent field is rendered as "не предоставлено", because a card that guesses
//! is worse than a card that admits a gap.

use crate::domain::{
    ConversationEntry, Lead, LeadRequirement, LeadStatus, RequirementField, RequirementScope,
};

/// Marker shown wherever a value was never provided.
pub const UNKNOWN: &str = "не предоставлено";

/// Everything the card is built from. Assembled by the caller, rendered here.
#[derive(Debug, Clone)]
pub struct ManagerCard<'a> {
    pub lead: &'a Lead,
    pub requirements: &'a [LeadRequirement],
    pub history: &'a [ConversationEntry],
    pub generated_at: chrono::DateTime<chrono::Utc>,
}

impl ManagerCard<'_> {
    fn get(&self, field: RequirementField) -> Option<String> {
        self.requirements
            .iter()
            .find(|r| r.field == field)
            .and_then(|r| r.value.as_deref())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    }

    fn need(&self, field: RequirementField) -> String {
        self.get(field).unwrap_or_else(|| UNKNOWN.to_string())
    }

    fn flag(&self, field: RequirementField) -> String {
        match self.get(field).as_deref() {
            Some(v) if matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "да" | "1") => {
                "да".to_string()
            }
            Some(_) => "нет".to_string(),
            None => UNKNOWN.to_string(),
        }
    }

    fn unit(&self, field: RequirementField) -> String {
        let unit_field = match field {
            RequirementField::GoodsWeight => Some(RequirementField::GoodsWeightUnit),
            RequirementField::GoodsVolume => Some(RequirementField::GoodsVolumeUnit),
            RequirementField::GoodsQuantity => Some(RequirementField::GoodsQuantityUnit),
            _ => None,
        };
        let from_column = self
            .requirements
            .iter()
            .find(|r| r.field == field)
            .and_then(|r| r.unit.clone())
            .unwrap_or_default();
        if !from_column.is_empty() {
            return from_column;
        }
        unit_field.and_then(|f| self.get(f)).unwrap_or_default()
    }

    fn quantity(&self, field: RequirementField) -> String {
        let value = self.need(field);
        let unit = self.unit(field);
        if value == UNKNOWN || unit.is_empty() {
            value
        } else {
            format!("{value} {unit}")
        }
    }

    fn route(&self) -> String {
        let from = [
            self.get(RequirementField::OriginCity),
            self.get(RequirementField::OriginCountry),
        ]
        .into_iter()
        .flatten()
        .next()
        .unwrap_or_else(|| UNKNOWN.to_string());
        let to = [
            self.get(RequirementField::DestinationCity),
            self.get(RequirementField::DestinationCountry),
        ]
        .into_iter()
        .flatten()
        .next()
        .unwrap_or_else(|| UNKNOWN.to_string());
        format!("{from} → {to}")
    }

    fn scope_label(scope: RequirementScope) -> &'static str {
        match scope {
            RequirementScope::Transport => "перевозка",
            RequirementScope::Customs => "таможенное оформление",
            RequirementScope::Procurement => "закупка за рубежом",
            RequirementScope::FullImport => "полное сопровождение импорта",
        }
    }

    fn status_label(status: LeadStatus) -> &'static str {
        match status {
            LeadStatus::New => "новый",
            LeadStatus::Processing => "в работе",
            LeadStatus::AwaitingCustomer => "ждём ответа клиента",
            LeadStatus::Qualified => "квалифицирован, готов менеджеру",
            LeadStatus::NeedsHumanReview => "нужен человек",
            LeadStatus::HandedOff => "передан менеджеру",
            LeadStatus::InProgress => "ведёт менеджер",
            LeadStatus::Won => "выигран",
            LeadStatus::Lost => "проигран",
            LeadStatus::Closed => "закрыт",
        }
    }

    /// Russian label for a blocking gap. `blocking_gaps` can only return a
    /// quote-blocking field, so the five arms below cover every value it
    /// produces and the rest fall back to the stable column name.
    fn gap_label(field: RequirementField) -> &'static str {
        match field {
            RequirementField::GoodsName => "наименование товара",
            RequirementField::OriginCountry => "страна отправления",
            RequirementField::DestinationCountry => "страна назначения",
            RequirementField::GoodsWeight => "вес",
            RequirementField::GoodsQuantity => "количество",
            other => other.as_str(),
        }
    }

    /// What still stands between this lead and a quote. A card that says only
    /// "в работе" is not actionable; this is the actionable half of `Статус`.
    fn gaps(&self) -> String {
        let missing = Lead::blocking_gaps(self.requirements);
        if missing.is_empty() {
            return "ничего — полный комплект".to_string();
        }
        missing
            .iter()
            .map(|f| Self::gap_label(*f))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn rows(&self) -> Vec<(&'static str, String)> {
        let weight = self.quantity(RequirementField::GoodsWeight);
        let volume = self.quantity(RequirementField::GoodsVolume);
        let currency = self
            .get(RequirementField::GoodsCurrency)
            .unwrap_or_default();
        let value = match self.get(RequirementField::GoodsValue) {
            Some(v) if currency.is_empty() => v,
            Some(v) => format!("{v} {currency}"),
            None => UNKNOWN.to_string(),
        };
        vec![
            ("Компания", self.need(RequirementField::CompanyName)),
            ("Контакт", self.need(RequirementField::ContactName)),
            ("Email", self.lead.contact_email.clone()),
            ("Телефон", self.need(RequirementField::ContactPhone)),
            ("Продукт / груз", self.need(RequirementField::GoodsName)),
            ("Вес", weight),
            ("Объём", volume),
            ("Маршрут", self.route()),
            ("Сроки", self.need(RequirementField::DesiredDeadline)),
            ("Транспорт", self.need(RequirementField::TransportMode)),
            ("Закупка", self.flag(RequirementField::NeedsProcurement)),
            (
                "Таможня",
                self.flag(RequirementField::NeedsCustomsClearance),
            ),
            ("Страхование", self.flag(RequirementField::NeedsInsurance)),
            ("Стоимость груза", value),
            (
                "Дополнительные условия",
                self.need(RequirementField::AdditionalRequirements),
            ),
            ("Услуги", Self::scope_label(self.lead.scope).to_string()),
            (
                "Исходная тема",
                self.history
                    .iter()
                    .rfind(|e| e.direction == crate::domain::ConversationDirection::Inbound)
                    .map(|e| e.subject.clone())
                    .unwrap_or_else(|| UNKNOWN.to_string()),
            ),
            ("Lead ID", self.lead.id.to_string()),
            (
                "Статус",
                format!(
                    "{} / {}",
                    Self::status_label(self.lead.status),
                    self.lead
                        .confidence
                        .map(|c| format!("confidence {c:.2}"))
                        .unwrap_or_else(|| format!("confidence {UNKNOWN}"))
                ),
            ),
            ("Чего не хватает", self.gaps()),
        ]
    }

    fn summary(&self) -> String {
        let head = self
            .lead
            .summary
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(UNKNOWN);
        let turns: Vec<String> = self
            .history
            .iter()
            .rev()
            .take(6)
            .map(|e| {
                let who = match e.direction {
                    crate::domain::ConversationDirection::Inbound => "Клиент",
                    crate::domain::ConversationDirection::Outbound => "MCA",
                };
                let body: String = e.body.chars().take(300).collect();
                format!("{who}: {body}")
            })
            .collect();
        if turns.is_empty() {
            head.to_string()
        } else {
            format!("{head}\n\nХод диалога:\n{}", turns.join("\n"))
        }
    }

    /// Plain-text card: what an email client without HTML shows.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("КАРТОЧКА КЛИЕНТА — MCA Logistics\n");
        out.push_str(&"=".repeat(48));
        out.push('\n');
        for (label, value) in self.rows() {
            out.push_str(&format!("{label:<24} {value}\n"));
        }
        out.push_str(&format!("\nРезюме диалога:\n{}\n", self.summary()));
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
             <h2 style=\"margin-bottom:4px\">Карточка клиента — MCA Logistics</h2>\
             <table style=\"border-collapse:collapse;width:100%;max-width:720px\">{rows}</table>\
             <h3>Резюме диалога</h3><p style=\"white-space:pre-wrap\">{summary}</p>\
             <p style=\"color:#777;font-size:12px\">Сформировано: {at}</p>\
             </body></html>",
            rows = rows,
            summary = escape(&self.summary()),
            at = escape(&self.generated_at.to_rfc3339()),
        )
    }
}

/// HTML escaping. Present because the card carries customer-controlled text
/// (company name, free-form conditions) straight into an HTML mail part.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[path = "manager_card_tests.rs"]
mod tests;
