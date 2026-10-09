use super::*;
use uuid::Uuid;

use crate::domain::{
    FieldState, LeadSource, RequirementField, RequirementScope, RequirementSource,
};

fn lead() -> Lead {
    let now = chrono::Utc::now();
    Lead {
        id: Uuid::new_v4(),
        thread_id: None,
        primary_email_id: None,
        source: LeadSource::Api,
        status: LeadStatus::Qualified,
        category: None,
        scope: RequirementScope::Transport,
        conversation_key: "voice:79161234567".into(),
        contact_email: "buyer@acme.test".into(),
        company_name: Some("ООО Ромашка".into()),
        company_inn: None,
        summary: Some("Нужна перевозка 12 тонн электроники из Самары в Москву".into()),
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
        evidence: None,
        updated_at: chrono::Utc::now(),
    }
}

fn card<'a>(reqs: &'a [LeadRequirement]) -> OrderCard<'a> {
    let lead = Box::leak(Box::new(lead()));
    OrderCard {
        lead,
        requirements: reqs,
        generated_at: chrono::Utc::now(),
    }
}

/// The value rendered for one row, with the label and padding stripped.
fn row(text: &str, label: &str) -> String {
    text.lines()
        .find_map(|l| l.split_once(label).map(|(_, rest)| rest))
        .map(|rest| rest.trim().to_string())
        .unwrap_or_else(|| panic!("no row labelled {label}"))
}

#[test]
fn absent_fields_render_as_missing_not_as_empty_string() {
    let text = card(&[]).to_text();
    assert_eq!(row(&text, "Компания"), UNKNOWN);
    assert_eq!(row(&text, "Телефон"), UNKNOWN);
    assert_eq!(row(&text, "Сроки"), UNKNOWN);
    assert_eq!(row(&text, "Маршрут"), format!("{UNKNOWN} → {UNKNOWN}"));
    assert_eq!(row(&text, "Email"), "buyer@acme.test");
}

#[test]
fn subject_prefers_the_goods_then_the_company_then_an_honest_generic() {
    let goods_reqs = [req(RequirementField::GoodsName, Some("Запчасти"), None)];
    let goods = card(&goods_reqs);
    assert_eq!(goods.subject(), "Карточка заказа: Запчасти — MCA Logistics");

    let company = card(&[]);
    assert_eq!(
        company.subject(),
        "Карточка заказа: ООО Ромашка — MCA Logistics"
    );

    let mut blank = lead();
    blank.company_name = None;
    let anonymous = OrderCard {
        lead: &blank,
        requirements: &[],
        generated_at: chrono::Utc::now(),
    };
    assert_eq!(
        anonymous.subject(),
        "Карточка заказа: заявка — MCA Logistics"
    );
}

#[test]
fn units_are_joined_to_the_quantity() {
    let reqs = [
        req(RequirementField::GoodsWeight, Some("12"), Some("т")),
        req(RequirementField::GoodsVolume, Some("48"), None),
        req(RequirementField::GoodsVolumeUnit, Some("м³"), None),
    ];
    let text = card(&reqs).to_text();
    assert_eq!(row(&text, "Вес"), "12 т");
    assert_eq!(row(&text, "Объём"), "48 м³");
}

#[test]
fn route_prefers_city_then_country() {
    let reqs = [
        req(RequirementField::OriginCity, Some("Самара"), None),
        req(RequirementField::DestinationCountry, Some("Китай"), None),
    ];
    assert_eq!(row(&card(&reqs).to_text(), "Маршрут"), "Самара → Китай");
}

#[test]
fn gaps_are_listed_in_russian_for_the_customer_to_act_on() {
    let reqs = [req(RequirementField::GoodsName, None, None)];
    let text = card(&reqs).to_text();
    assert_eq!(
        row(&text, "Чего не хватает для расчёта"),
        "наименование товара"
    );

    let full = card(&[]).to_text();
    assert_eq!(
        row(&full, "Чего не хватает для расчёта"),
        "ничего — полный комплект"
    );
}

#[test]
fn html_escapes_customer_controlled_text() {
    let reqs = [req(
        RequirementField::CompanyName,
        Some("<script>alert(1)</script>"),
        None,
    )];
    let html = card(&reqs).to_html();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("buyer@acme.test"));
}

#[test]
fn both_representations_carry_the_same_row_set() {
    let reqs = [req(RequirementField::ContactName, Some("Иван"), None)];
    let c = card(&reqs);
    let text = c.to_text();
    let html = c.to_html();
    for label in [
        "Номер обращения",
        "Статус",
        "Маршрут",
        "Чего не хватает для расчёта",
        "Email",
    ] {
        assert!(text.contains(label), "text missing {label}");
        assert!(html.contains(label), "html missing {label}");
    }
    assert!(text.contains("Иван"));
    assert!(html.contains("Иван"));
}

#[test]
fn nothing_internal_leaks_to_the_customer() {
    let text = card(&[]).to_text();
    // No confidence score, no CRM lead-id wording, no dialogue transcript.
    assert!(!text.contains("confidence"));
    assert!(!text.contains("Lead ID"));
    assert!(!text.contains("Ход диалога"));
    assert!(!text.contains("квалифицирован, готов менеджеру"));
    // The status speaks the customer's language instead.
    assert!(text.contains("данные собраны, готовим расчёт"));
}

#[test]
fn summary_comes_from_the_saved_summary_only() {
    let text = card(&[]).to_text();
    assert!(text.contains("Нужна перевозка 12 тонн электроники из Самары в Москву"));

    let mut blank = lead();
    blank.summary = None;
    let anonymous = OrderCard {
        lead: &blank,
        requirements: &[],
        generated_at: chrono::Utc::now(),
    };
    assert!(anonymous.to_text().contains(&format!("Резюме:\n{UNKNOWN}")));
}

#[test]
fn card_identifies_the_request() {
    let c = card(&[]);
    assert!(c.to_text().contains(&c.lead.id.to_string()));
    assert!(c.to_text().contains("КАРТОЧКА ЗАКАЗА — MCA Logistics"));
}
