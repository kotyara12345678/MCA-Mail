//! Unit tests for `api-key` argument parsing and formatting helpers.

use super::*;
use crate::persistence::api_key_repo::Role;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parse_create_requires_name_and_role() {
    let err = parse_create(&args(&["--role", "manager"])).unwrap_err();
    assert!(err.to_string().contains("--name is required"));

    let err = parse_create(&args(&["--name", "ops"])).unwrap_err();
    assert!(err.to_string().contains("--role is required"));
}

#[test]
fn parse_create_accepts_all_flags() {
    let parsed = parse_create(&args(&[
        "--name",
        "nightly ops",
        "--role",
        "Admin",
        "--expires-days",
        "90",
        "--created-by",
        "alice",
    ]))
    .unwrap();
    assert_eq!(parsed.name, "nightly ops");
    assert_eq!(parsed.role, Role::Admin);
    assert_eq!(parsed.expires_days, Some(90));
    assert_eq!(parsed.created_by, "alice");
    assert!(Role::Admin.at_least(Role::Admin));
}

#[test]
fn parse_create_rejects_bad_role_and_days() {
    let err = parse_create(&args(&["--name", "x", "--role", "root"])).unwrap_err();
    assert!(err.to_string().contains("invalid role"));

    let err = parse_create(&args(&[
        "--name",
        "x",
        "--role",
        "viewer",
        "--expires-days",
        "soon",
    ]))
    .unwrap_err();
    assert!(err.to_string().contains("whole number"));
}

#[test]
fn parse_create_rejects_unknown_and_dangling_flags() {
    let err = parse_create(&args(&["--name", "x", "--role", "viewer", "--nope"])).unwrap_err();
    assert!(err.to_string().contains("unexpected argument"));

    let err = parse_create(&args(&["--name"])).unwrap_err();
    assert!(err.to_string().contains("`--name` requires a value"));
}

#[test]
fn fit_truncates_on_char_boundary() {
    assert_eq!(fit("plain", 20), "plain");
    assert_eq!(fit("очень длинное имя проекта", 10), "очень дли…");
    assert_eq!(fit("12345", 1), "…");
}
