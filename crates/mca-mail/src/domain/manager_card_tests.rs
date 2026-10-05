use super::*;
use uuid::Uuid;

use crate::domain::{
    ConversationDirection, FieldState, LeadSource, OutboundState, RequirementSource,
};

fn lead() -> Lead {
    let now = chrono::Utc::now();
    Lead {
        id: Uuid::new_v4(),
        thread_id: None,
        primary_email_id: None,
        source: LeadSource::InboundEmail,
        status: LeadStatus::Qualified,
        category: None,
        scope: RequirementScope::Transport,
        conversation_key: "abc".into(),
        contact_email: "buyer@acme.test".into(),
        company_name: Some("ООО Ромашка".into()),
        company_inn: None,
        summary: Some("Нужна перевозка 12 тонн электроники из Испании в Москву".into()),
        confidence: Some(0.81),
        open_questions: vec![],
        unresolved_topics: vec![],
        callback_requested: false,
        callback_phone: None,
        automation_released_by: None,
        last_activity_at: now,
        created_at: now,
        updated_at: now,
    }
}

fn req(field: RequirementField, value: Option<&str>, unit: Option<&str>) -> LeadRequirement {
    LeadRequirement {
        lead_id: Uuid::nil(),
        field,
        value: value.map(str::to_string),
        state: if value.is_some() {
            FieldState::Known
        } else {
            FieldState::Unknown
        },
        source: RequirementSource::Customer,
        unit: unit.map(str::to_string),
        confidence: Some(0.9),
        evidence: value.map(|v| format!("...{v}...")),
        updated_at: chrono::Utc::now(),
    }
}

fn entry(direction: ConversationDirection, subject: &str, body: &str) -> ConversationEntry {
    ConversationEntry {
        id: Uuid::new_v4(),
        lead_id: Uuid::nil(),
        email_id: None,
        direction,
        state: OutboundState::Sent,
        subject: subject.to_string(),
        body: body.to_string(),
        sent_at: None,
        created_at: chrono::Utc::now(),
        idempotency_key: "k".into(),
    }
}

fn card<'a>(reqs: &'a [LeadRequirement], hist: &'a [ConversationEntry]) -> ManagerCard<'a> {
    let lead = Box::leak(Box::new(lead()));
    ManagerCard {
        lead,
        requirements: reqs,
        history: hist,
        generated_at: chrono::Utc::now(),
    }
}

/// The value rendered for one row, with the label and padding stripped.
/// Tests compare content, not column widths: re-flowing the card must not
/// break them.
fn row(text: &str, label: &str) -> String {
    text.lines()
        .find_map(|l| l.split_once(label).map(|(_, rest)| rest))
        .map(|rest| rest.trim().to_string())
        .unwrap_or_else(|| panic!("no row labelled {label}"))
}

#[test]
fn absent_fields_render_as_missing_not_as_empty_string() {
    let c = card(&[], &[]);
    let text = c.to_text();
    assert_eq!(row(&text, "Компания"), UNKNOWN);
    assert_eq!(row(&text, "Телефон"), UNKNOWN);
    assert_eq!(row(&text, "Маршрут"), format!("{UNKNOWN} → {UNKNOWN}"));
    assert_eq!(row(&text, "Страхование"), UNKNOWN);
    assert_eq!(row(&text, "Сроки"), UNKNOWN);
    assert_eq!(row(&text, "Стоимость груза"), UNKNOWN);
}

#[test]
fn units_are_joined_to_the_quantity() {
    let reqs = [
        req(RequirementField::GoodsWeight, Some("12"), Some("т")),
        req(RequirementField::GoodsVolume, Some("48"), None),
        req(RequirementField::GoodsVolumeUnit, Some("м³"), None),
    ];
    let c = card(&reqs, &[]);
    assert_eq!(row(&c.to_text(), "Вес"), "12 т");
    assert_eq!(row(&c.to_text(), "Объём"), "48 м³");
}

#[test]
fn route_prefers_city_then_country() {
    let reqs = [
        req(RequirementField::OriginCity, Some("Самара"), None),
        req(RequirementField::DestinationCountry, Some("Китай"), None),
    ];
    let c = card(&reqs, &[]);
    assert_eq!(row(&c.to_text(), "Маршрут"), "Самара → Китай");
}

#[test]
fn boolean_flags_speak_yes_or_no_rather_than_a_raw_literal() {
    let yes = [req(
        RequirementField::NeedsCustomsClearance,
        Some("true"),
        None,
    )];
    let no = [req(RequirementField::NeedsProcurement, Some("false"), None)];
    assert_eq!(row(&card(&yes, &[]).to_text(), "Таможня"), "да");
    assert_eq!(row(&card(&no, &[]).to_text(), "Закупка"), "нет");
}

#[test]
fn html_escapes_customer_controlled_text() {
    let reqs = [req(
        RequirementField::CompanyName,
        Some("<script>alert(1)</script>"),
        None,
    )];
    let c = card(&reqs, &[]);
    let html = c.to_html();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("buyer@acme.test"));
}

#[test]
fn both_representations_carry_the_same_row_set() {
    let reqs = [req(
        RequirementField::ContactName,
        Some("Иван Петров"),
        None,
    )];
    let c = card(&reqs, &[]);
    let text = c.to_text();
    let html = c.to_html();
    for label in ["Компания", "Маршрут", "Сроки", "Lead ID", "Статус"] {
        assert!(text.contains(label), "text missing {label}");
        assert!(html.contains(label), "html missing {label}");
    }
    assert!(text.contains("Иван Петров"));
    assert!(html.contains("Иван Петров"));
}

#[test]
fn summary_quotes_the_last_inbound_subject_and_recent_turns() {
    let history = [
        entry(
            ConversationDirection::Inbound,
            "Запрос на перевозку",
            "Здравствуйте, нужна машина из Самары в Владивосток",
        ),
        entry(
            ConversationDirection::Outbound,
            "Re: Запрос на перевозку",
            "Сколько груза и какие сроки?",
        ),
        entry(
            ConversationDirection::Inbound,
            "Re: Запрос на перевозку",
            "12 тонн, до 20 марта",
        ),
    ];
    let c = card(&[], &history);
    let text = c.to_text();
    assert_eq!(row(&text, "Исходная тема"), "Re: Запрос на перевозку");
    assert!(text.contains("Ход диалога:"));
    assert!(text.contains("Клиент: 12 тонн, до 20 марта"));
    assert!(text.contains("MCA: Сколько груза и какие сроки?"));
}

#[test]
fn summary_falls_back_to_the_lead_summary_when_history_is_empty() {
    let c = card(&[], &[]);
    assert!(c
        .to_text()
        .contains("Нужна перевозка 12 тонн электроники из Испании в Москву"));
}

#[test]
fn card_identifies_the_lead_and_reports_confidence() {
    let text = card(&[], &[]).to_text();
    assert!(text.contains("Lead ID"));
    assert!(text.contains("confidence 0.81"));
    assert!(text.contains("квалифицирован, готов менеджеру"));
}

#[test]
fn missing_confidence_is_admitted_rather_than_printed_as_zero() {
    let mut l = lead();
    l.confidence = None;
    let c = ManagerCard {
        lead: &l,
        requirements: &[],
        history: &[],
        generated_at: chrono::Utc::now(),
    };
    let text = c.to_text();
    assert!(text.contains(&format!("confidence {UNKNOWN}")));
    assert!(!text.contains("confidence 0.00"));
}
