//! Unit tests for API key hashing, ranking and lookup helpers.

use super::*;

#[test]
fn role_ranking_is_ordered() {
    assert!(Role::Admin.at_least(Role::Viewer));
    assert!(Role::Manager.at_least(Role::Operator));
    assert!(!Role::Operator.at_least(Role::Manager));
    assert!(!Role::Viewer.at_least(Role::Operator));
}

#[test]
fn keys_are_prefixed_and_hashed_deterministically() {
    let key = generate_key();
    assert!(key.starts_with("mca_"));
    assert_eq!(hash_key(&key), hash_key(&key));
    assert_ne!(hash_key(&key), hash_key(&generate_key()));
    assert_eq!(key_prefix(&key).len(), 8);
    assert!(!hash_key(&key).contains(&key));
}
