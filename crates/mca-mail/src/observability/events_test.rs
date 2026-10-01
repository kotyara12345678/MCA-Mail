//! Event helper tests: lifecycle, actions, agent and LLM emissions.

use super::test_util::{await_marker, run};
use super::{actions, agents, bus, lifecycle, llm, Correlation};

fn corr() -> Correlation {
    Correlation::new(uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
}

#[test]
fn lifecycle_processing_events_carry_correlation() {
    let mut rx = bus::subscribe();
    let c = corr();
    run(|| {
        lifecycle::processing_started(&c);
        lifecycle::processing_completed(&c, "processed", 42);
        lifecycle::processing_failed(&c, "llm", "provider exploded", 2);
        lifecycle::email_received(c.email_id, uuid::Uuid::new_v4(), Some("msg-1"));
    });
    let json = |ev: &bus::BusEvent| serde_json::from_str::<serde_json::Value>(&ev.json).unwrap();

    let e1 = await_marker(&mut rx, &c.processing_id);
    assert_eq!(e1.event, "processing_started");
    let v = json(&e1);
    assert_eq!(v["processing_id"], c.processing_id);

    let e2 = await_marker(&mut rx, &c.processing_id);
    assert_eq!(e2.event, "processing_completed");
    let v = json(&e2);
    assert_eq!(v["status"], "processed");
    assert_eq!(v["duration_ms"], 42);

    let e3 = await_marker(&mut rx, &c.processing_id);
    assert_eq!(e3.event, "processing_failed");
    let v = json(&e3);
    assert_eq!(v["error_type"], "llm");
    assert_eq!(v["retry_count"], 2);

    let e4 = await_marker(&mut rx, "msg-1");
    assert_eq!(e4.event, "email_received");
}

#[test]
fn actions_and_agent_events_include_ids_and_results() {
    let mut rx = bus::subscribe();
    let c = corr();
    let draft_id = uuid::Uuid::new_v4();
    run(|| {
        actions::draft_created(&c, draft_id, uuid::Uuid::new_v4(), "draft");
        actions::handoff_created(&c, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "callback");
        actions::email_moved(Some(c.email_id), "folder-m1");
        actions::email_labeled(None, "lead");
        agents::agent_started(&c, "spam");
        agents::agent_completed(&c, "spam", "not_spam", 731);
        agents::agent_failed(&c, "classification", "agent", "bad output", 12);
    });
    let json = |ev: &bus::BusEvent| serde_json::from_str::<serde_json::Value>(&ev.json).unwrap();

    let e = await_marker(&mut rx, &c.processing_id);
    assert_eq!(e.event, "draft_created");
    assert_eq!(json(&e)["draft_id"], draft_id.to_string());

    let e = await_marker(&mut rx, &c.processing_id);
    assert_eq!(e.event, "handoff_created");
    let e = await_marker(&mut rx, "folder-m1");
    assert_eq!(e.event, "email_moved");
    let e = await_marker(&mut rx, "\"event\":\"email_labeled\"");
    assert_eq!(e.event, "email_labeled");
    let e = await_marker(&mut rx, &c.processing_id);
    assert_eq!(e.event, "agent_started");
    let e = await_marker(&mut rx, &c.processing_id);
    assert_eq!(e.event, "agent_completed");
    assert_eq!(json(&e)["result"], "not_spam");
    let e = await_marker(&mut rx, &c.processing_id);
    assert_eq!(e.event, "agent_failed");
}

#[test]
fn llm_events_expose_usage_and_retries_but_never_prompts() {
    let mut rx = bus::subscribe();
    let c = corr();
    run(|| {
        llm::llm_request_started(&c, "spam", "polza", "deepseek-v4");
        llm::llm_request_completed(&c, "spam", "polza", "deepseek-v4", 810, 500, 60);
        llm::llm_retry("polza", "deepseek-v4", 1, "http 502");
        llm::llm_failed("polza", "deepseek-v4", "http", 2, "http 500");
    });
    let json = |ev: &bus::BusEvent| serde_json::from_str::<serde_json::Value>(&ev.json).unwrap();

    let e = await_marker(&mut rx, "deepseek-v4");
    assert_eq!(e.event, "llm_request_started");
    let e = await_marker(&mut rx, "deepseek-v4");
    assert_eq!(e.event, "llm_request_completed");
    let v = json(&e);
    assert_eq!(v["input_tokens"], 500);
    assert_eq!(v["output_tokens"], 60);
    assert_eq!(v["total_tokens"], 560);
    let e = await_marker(&mut rx, "http 502");
    assert_eq!(e.event, "llm_retry");
    let e = await_marker(&mut rx, "http 500");
    assert_eq!(e.event, "llm_failed");
    assert_eq!(json(&e)["retry_count"], 2);
    assert!(!e.json.contains("system_prompt"));
}
