use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{LeadId, ResearchStatus};
use crate::error::AppError;

/// Persist a company pre-check.
///
/// A report is only ever written from a provider response; a `not_configured`
/// status is stored as an explicit fact so the UI can distinguish "not looked
/// up" from "looked up, nothing found".
pub async fn save(
    pool: &PgPool,
    report: &crate::domain::CompanyResearchReport,
    identifier: &str,
    ttl_seconds: i64,
) -> Result<Uuid, AppError> {
    let company = report.company.as_ref();
    let expires_at = if report.status.has_data() {
        chrono::Utc::now() + chrono::Duration::seconds(ttl_seconds.max(0))
    } else {
        chrono::Utc::now()
    };
    let sql = "INSERT INTO company_research (lead_id, identifier, status, inn, ogrn, legal_name, \
         company_state, registered_at, region, main_activity, management, liquidation_signs, \
         court_cases, enforcement_cases, revenue_last_year, profit_last_year, completeness, \
         sources, gaps, flags, notes, performed_at, expires_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22) \
         ON CONFLICT (identifier, COALESCE(lead_id, '00000000-0000-0000-0000-000000000000')) \
         DO UPDATE SET status = EXCLUDED.status, inn = EXCLUDED.inn, ogrn = EXCLUDED.ogrn, \
         legal_name = EXCLUDED.legal_name, company_state = EXCLUDED.company_state, \
         registered_at = EXCLUDED.registered_at, region = EXCLUDED.region, \
         main_activity = EXCLUDED.main_activity, management = EXCLUDED.management, \
         liquidation_signs = EXCLUDED.liquidation_signs, court_cases = EXCLUDED.court_cases, \
         enforcement_cases = EXCLUDED.enforcement_cases, \
         revenue_last_year = EXCLUDED.revenue_last_year, \
         profit_last_year = EXCLUDED.profit_last_year, completeness = EXCLUDED.completeness, \
         sources = EXCLUDED.sources, gaps = EXCLUDED.gaps, flags = EXCLUDED.flags, \
         notes = EXCLUDED.notes, performed_at = EXCLUDED.performed_at, \
         expires_at = EXCLUDED.expires_at \
         RETURNING id";
    let id = sqlx::query_scalar::<_, Uuid>(sql)
        .bind(report.lead_id)
        .bind(identifier)
        .bind(report.status.as_str())
        .bind(&report.identifiers.inn)
        .bind(&report.identifiers.ogrn)
        .bind(&report.identifiers.legal_name)
        .bind(company.map(|c| c.state.clone()))
        .bind(company.and_then(|c| c.registered_at))
        .bind(company.and_then(|c| c.registration_region.clone()))
        .bind(company.and_then(|c| c.main_activity.clone()))
        .bind(company.and_then(|c| c.management.clone()))
        .bind(company.and_then(|c| c.has_liquidation_or_bankruptcy_signs))
        .bind(company.and_then(|c| c.court_cases_count))
        .bind(company.and_then(|c| c.enforcement_proceedings_count))
        .bind(company.and_then(|c| c.revenue_last_year))
        .bind(company.and_then(|c| c.profit_last_year))
        .bind(report.completeness as f64)
        .bind(serde_json::to_value(&report.sources).unwrap_or(serde_json::Value::Null))
        .bind(serde_json::to_value(&report.gaps).unwrap_or(serde_json::Value::Null))
        .bind(serde_json::to_value(&report.flags).unwrap_or(serde_json::Value::Null))
        .bind(&report.notes)
        .bind(report.performed_at)
        .bind(expires_at)
        .fetch_one(pool)
        .await?;
    Ok(id)
}

/// Return a cached report when it is still fresh.
pub async fn find_fresh(
    pool: &PgPool,
    identifier: &str,
    lead_id: Option<LeadId>,
) -> Result<Option<crate::domain::CompanyResearchReport>, AppError> {
    let row = sqlx::query_as::<_, ResearchRow>(
        "SELECT status, inn, ogrn, legal_name, company_state, registered_at, region, \
         main_activity, management, liquidation_signs, court_cases, enforcement_cases, \
         revenue_last_year, profit_last_year, completeness, sources, gaps, flags, notes, \
         performed_at, expires_at FROM company_research \
         WHERE identifier = $1 AND expires_at > now() \
         AND COALESCE(lead_id, '00000000-0000-0000-0000-000000000000') = \
             COALESCE($2, '00000000-0000-0000-0000-000000000000')",
    )
    .bind(identifier)
    .bind(lead_id)
    .fetch_optional(pool)
    .await?;
    row.map(ResearchRow::into_domain).transpose()
}

#[derive(Debug, sqlx::FromRow)]
struct ResearchRow {
    status: String,
    inn: Option<String>,
    ogrn: Option<String>,
    legal_name: Option<String>,
    company_state: Option<String>,
    registered_at: Option<chrono::DateTime<chrono::Utc>>,
    region: Option<String>,
    main_activity: Option<String>,
    management: Option<String>,
    liquidation_signs: Option<bool>,
    court_cases: Option<i32>,
    enforcement_cases: Option<i32>,
    revenue_last_year: Option<f64>,
    profit_last_year: Option<f64>,
    completeness: f64,
    sources: serde_json::Value,
    gaps: serde_json::Value,
    flags: serde_json::Value,
    notes: Option<String>,
    performed_at: Option<chrono::DateTime<chrono::Utc>>,
    #[allow(dead_code)]
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl ResearchRow {
    fn into_domain(self) -> Result<crate::domain::CompanyResearchReport, AppError> {
        use crate::domain::{
            CompanyIdentifiers, CompanyResearchReport, CompanyStatus, ResearchSource,
        };
        let status: ResearchStatus = self
            .status
            .parse()
            .map_err(|e: crate::domain::WireParseError| sqlx::Error::Decode(Box::new(e)))?;
        let company = if self.company_state.is_some() || self.registered_at.is_some() {
            Some(CompanyStatus {
                state: self.company_state.unwrap_or_else(|| "unknown".into()),
                registered_at: self.registered_at,
                registration_region: self.region,
                main_activity: self.main_activity,
                management: self.management,
                has_liquidation_or_bankruptcy_signs: self.liquidation_signs,
                court_cases_count: self.court_cases,
                enforcement_proceedings_count: self.enforcement_cases,
                revenue_last_year: self.revenue_last_year,
                profit_last_year: self.profit_last_year,
            })
        } else {
            None
        };
        Ok(CompanyResearchReport {
            id: Uuid::nil(),
            lead_id: None,
            status,
            identifiers: CompanyIdentifiers {
                inn: self.inn,
                ogrn: self.ogrn,
                legal_name: self.legal_name,
            },
            company,
            completeness: self.completeness as f32,
            sources: serde_json::from_value::<Vec<ResearchSource>>(self.sources)
                .unwrap_or_default(),
            gaps: serde_json::from_value::<Vec<String>>(self.gaps).unwrap_or_default(),
            flags: serde_json::from_value::<Vec<String>>(self.flags).unwrap_or_default(),
            notes: self.notes,
            performed_at: self.performed_at,
        })
    }
}

pub async fn count(pool: &PgPool) -> Result<i64, AppError> {
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM company_research")
        .fetch_one(pool)
        .await?;
    Ok(total)
}
