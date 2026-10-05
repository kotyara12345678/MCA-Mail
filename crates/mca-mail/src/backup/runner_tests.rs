use super::{parse_url, restore_list_args};
use std::path::Path;

#[test]
fn restore_validation_requests_the_archive_table_of_contents() {
    let url = url::Url::parse("postgres://user:secret@localhost:5432/mail").unwrap();
    let connection = parse_url(&url).unwrap();
    let args = restore_list_args(&connection, Path::new("archive.dump"));

    assert!(args.contains(&"--list".to_string()));
    assert!(args.contains(&"archive.dump".to_string()));
    assert!(!args.iter().any(|arg| arg.contains("secret")));
}
