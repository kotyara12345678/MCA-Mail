//! Unit tests for the SSE broadcast layer.

use std::time::Duration;

use super::bus::subscribe;
use super::test_util::{assert_absent, await_marker, run};
use super::EVENT_TARGET;

#[test]
fn obs_events_reach_subscribers_with_json_payload() {
    let mut rx = subscribe();
    run(|| {
        tracing::info!(
            target: EVENT_TARGET,
            email_id = "bus-e1",
            agent = "spam",
            duration_ms = 5u64,
            ok = true,
            "agent_completed"
        );
    });
    let ev = await_marker(&mut rx, "bus-e1");
    assert_eq!(ev.event, "agent_completed");
    let v: serde_json::Value = serde_json::from_str(&ev.json).expect("valid json");
    assert_eq!(v["event"], "agent_completed");
    assert_eq!(v["level"], "INFO");
    assert_eq!(v["email_id"], "bus-e1");
    assert_eq!(v["agent"], "spam");
    assert_eq!(v["duration_ms"], 5);
    assert_eq!(v["ok"], true);
    assert!(v["ts"].as_str().unwrap().contains('T'), "{}", v["ts"]);
}

#[test]
fn non_obs_events_never_reach_the_bus() {
    let mut rx = subscribe();
    run(|| tracing::info!(marker = "plain-only-9f3", "plain application log"));
    assert_absent(&mut rx, "plain-only-9f3", Duration::from_millis(300));
}

#[test]
fn debug_and_typed_values_are_captured() {
    let mut rx = subscribe();
    run(|| {
        tracing::debug!(target: EVENT_TARGET, count = -3i64, marker = "dbg-e1", "stuck_emails_found");
    });
    let ev = await_marker(&mut rx, "dbg-e1");
    let v: serde_json::Value = serde_json::from_str(&ev.json).unwrap();
    assert_eq!(v["count"], -3);
    assert_eq!(v["level"], "DEBUG");
}
