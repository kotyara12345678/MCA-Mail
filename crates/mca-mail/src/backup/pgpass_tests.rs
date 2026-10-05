use super::*;

fn url(text: &str) -> url::Url {
    url::Url::parse(text).unwrap()
}

#[test]
fn pgpass_line_targets_host_port_database_and_user() {
    let line = authority_prefix(&url("postgres://mca:s3cret@db.internal:6543/mca_mail"));
    assert_eq!(line, "db.internal:6543:mca_mail:mca");
    assert!(!line.contains("s3cret"));
}

#[test]
fn a_query_string_does_not_break_the_match() {
    let line = authority_prefix(&url(
        "postgres://mca:s3cret@localhost:5432/db?sslmode=require",
    ));
    assert_eq!(line, "localhost:5432:db:mca");
}

/// libpq reads only a five-field line and silently ignores anything else, so a
/// wrong field count does not fail here — it fails later as `no password
/// supplied` out of `pg_dump`. This is the assertion that catches it.
#[test]
fn the_written_line_is_exactly_five_fields() {
    let dir = std::env::temp_dir().join(format!("pgpass-shape-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = write_pgpass(&dir, &url("postgres://mca:s3cret@localhost:5432/db")).unwrap();
    let line = std::fs::read_to_string(&path).unwrap();
    let line = line.trim_end();
    assert_eq!(line, "localhost:5432:db:mca:s3cret");
    assert_eq!(
        line.split(':').count(),
        5,
        "not a libpq password line: {line}"
    );
    drop(PgPassGuard::new(path));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_url_without_password_is_refused() {
    let err = password_of(&url("postgres://mca@localhost/db")).expect_err("no password");
    assert!(matches!(err, BackupError::Config(_)));
}

#[test]
fn the_file_is_removed_when_the_guard_drops() {
    let dir = std::env::temp_dir().join(format!("pgpass-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = write_pgpass(&dir, &url("postgres://mca:s3cret@localhost:5432/db")).unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("s3cret"));
    drop(PgPassGuard::new(path));
    assert!(!dir.join(".pgpass").exists());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn existing_pgpass_is_never_overwritten() {
    let dir = std::env::temp_dir().join(format!("pgpass-existing-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(".pgpass");
    std::fs::write(&path, b"preserve this file").unwrap();
    assert!(write_pgpass(&dir, &url("postgres://mca:s3cret@localhost:5432/db")).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"preserve this file");
    std::fs::remove_dir_all(dir).ok();
}
