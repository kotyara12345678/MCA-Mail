use super::{nested_target, TABLES};
use std::collections::HashSet;

#[test]
fn flat_keys_are_unique() {
    let mut seen = HashSet::new();
    for table in TABLES {
        for (flat, _) in table.iter() {
            assert!(seen.insert(*flat), "duplicate flat key {flat}");
        }
    }
}

#[test]
fn nested_targets_are_unique() {
    let mut seen = HashSet::new();
    for table in TABLES {
        for (_, nested) in table.iter() {
            assert!(seen.insert(*nested), "duplicate nested key {nested}");
        }
    }
}

#[test]
fn lookup_resolves_documented_names() {
    assert_eq!(nested_target("DATABASE_URL"), Some("database.url"));
    assert_eq!(nested_target("MAIL_IMAP_PORT"), Some("mail.imap.port"));
    assert_eq!(nested_target("EMAIL_MODE"), Some("security.email_mode"));
    assert_eq!(nested_target("PATH"), None);
    assert_eq!(nested_target("NOT_A_REAL_KEY"), None);
}
