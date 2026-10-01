use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ids::ResearchId;

crate::domain::wire_enum! {
    /// Explicitly distinguishes "we looked and found nothing" from "we could
    /// not look at all". The API never reports an unconfigured provider as a
    /// successful check.
    ResearchStatus {
        NotConfigured => "not_configured",
        Pending => "pending",
        Completed => "completed",
        Partial => "partial",
        Failed => "failed",
        /// No identifier available to search with (e.g. no INN and no name).
        NoIdentifier => "no_identifier",
    }
}

impl ResearchStatus {
    pub const fn has_data(&self) -> bool {
        matches!(self, ResearchStatus::Completed | ResearchStatus::Partial)
    }
}

/// A single cited fact. Every value the agent reports must carry provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchSource {
    /// e.g. `configured_registry`, `manual_entry`.
    pub source: String,
    pub fetched_at: DateTime<Utc>,
    /// Human readable locator: registry name, URL or request identifier.
    pub locator: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompanyIdentifiers {
    pub inn: Option<String>,
    pub ogrn: Option<String>,
    pub legal_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompanyStatus {
    /// `active`, `liquidation`, `bankruptcy`, `unknown`.
    pub state: String,
    pub registered_at: Option<DateTime<Utc>>,
    pub registration_region: Option<String>,
    pub main_activity: Option<String>,
    pub management: Option<String>,
    pub has_liquidation_or_bankruptcy_signs: Option<bool>,
    pub court_cases_count: Option<i32>,
    pub enforcement_proceedings_count: Option<i32>,
    /// Partial financial information, only if a source provides it.
    pub revenue_last_year: Option<f64>,
    pub profit_last_year: Option<f64>,
}

/// Result of a company pre-check. Fields that no source provided stay `None`
/// and are reported as gaps, never filled with plausible guesses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompanyResearchReport {
    pub id: ResearchId,
    pub lead_id: Option<super::ids::LeadId>,
    pub status: ResearchStatus,
    pub identifiers: CompanyIdentifiers,
    pub company: Option<CompanyStatus>,
    /// `0.0..=1.0` share of the requested fields actually obtained.
    pub completeness: f32,
    pub sources: Vec<ResearchSource>,
    /// Fields that need a human or another source.
    pub gaps: Vec<String>,
    pub flags: Vec<String>,
    pub notes: Option<String>,
    pub performed_at: Option<DateTime<Utc>>,
}

impl CompanyResearchReport {
    pub fn not_configured(lead_id: Option<super::ids::LeadId>, reason: &str) -> Self {
        Self {
            id: ResearchId::nil(),
            lead_id,
            status: ResearchStatus::NotConfigured,
            identifiers: CompanyIdentifiers {
                inn: None,
                ogrn: None,
                legal_name: None,
            },
            company: None,
            completeness: 0.0,
            sources: Vec::new(),
            gaps: vec![reason.to_string()],
            flags: Vec::new(),
            notes: Some("Проверка не выполнялась: внешний источник не подключён.".to_string()),
            performed_at: None,
        }
    }

    pub fn no_identifier() -> Self {
        Self {
            status: ResearchStatus::NoIdentifier,
            gaps: vec!["Не передан ИНН и не найдено наименование компании".to_string()],
            ..Self::not_configured(None, "нет идентификатора для поиска")
        }
    }

    /// Render for a manager handoff: sources always listed, gaps always listed.
    pub fn digest(&self) -> String {
        let mut lines = vec![format!("status: {}", self.status.as_str())];
        if self.status == ResearchStatus::NotConfigured {
            lines.push("проверка не выполнялась".to_string());
        }
        if let Some(inn) = &self.identifiers.inn {
            lines.push(format!("ИНН: {inn}"));
        }
        if let Some(legal) = &self.identifiers.legal_name {
            lines.push(format!("Наименование: {legal}"));
        }
        if let Some(c) = &self.company {
            lines.push(format!("Состояние: {}", c.state));
            if let Some(region) = &c.registration_region {
                lines.push(format!("Регион: {region}"));
            }
        }
        for src in &self.sources {
            lines.push(format!("Источник: {} ({})", src.source, src.locator));
        }
        for gap in &self.gaps {
            lines.push(format!("Не получено: {gap}"));
        }
        for flag in &self.flags {
            lines.push(format!("Требует проверки: {flag}"));
        }
        lines.push(format!("Полнота: {:.0}%", self.completeness * 100.0));
        lines.join("\n")
    }
}
