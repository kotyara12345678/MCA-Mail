use super::define_move_email;

#[test]
fn move_tool_accepts_roles_not_imap_folder_names() {
    let schema = define_move_email().arg_schema;
    assert!(schema["properties"]["role"]["enum"].is_array());
    assert!(schema["properties"]["folder"].is_null());
    assert_eq!(schema["required"][1], "role");
}
