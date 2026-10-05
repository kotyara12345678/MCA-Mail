use mca_mail::backup::BackupName;

#[test]
fn the_password_never_appears_in_the_argument_vector() {
    let parsed =
        url::Url::parse("postgres://mca_user:P4ssw0rd-do-not-log@db.internal:6543/mca_mail")
            .expect("url");
    let secret = parsed.password().expect("password").to_string();
    let args = mca_mail::backup::connection_arguments(&parsed)
        .expect("arguments")
        .join(" ");
    assert!(!args.contains(&secret));
    assert!(args.contains("--host db.internal"));
    assert!(args.contains("--port 6543"));
    assert!(args.contains("--username mca_user"));
}

#[test]
fn file_names_sort_oldest_first() {
    let timestamps = ["2025-12-31T23:59:59Z", "2026-01-01T00:00:00Z"];
    let names: Vec<_> = timestamps
        .iter()
        .map(|stamp| {
            let time = chrono::DateTime::parse_from_rfc3339(stamp)
                .unwrap()
                .with_timezone(&chrono::Utc);
            BackupName::new(time).file_name()
        })
        .collect();
    assert!(names[0] < names[1]);
}
