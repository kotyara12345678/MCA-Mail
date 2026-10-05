use async_imap::types::NameAttribute;

use super::{resolve, FolderInfo, FolderRole};

#[test]
fn names_do_not_imply_special_use_roles() {
    let folder = FolderInfo::from_list("INBOX_Drafts", &[]);
    assert_eq!(folder.role, FolderRole::Other);
    assert!(folder.special_use.is_empty());
}

#[test]
fn duplicate_roles_are_not_resolved() {
    let draft = FolderInfo::from_list("Drafts", &[NameAttribute::Drafts]);
    let other = FolderInfo::from_list("INBOX_Drafts", &[NameAttribute::Drafts]);
    assert_eq!(resolve(FolderRole::Drafts, &[draft, other]), None);
}

#[test]
fn inbox_is_the_only_name_based_role() {
    let inbox = FolderInfo::from_list("INBOX", &[]);
    assert_eq!(inbox.role, FolderRole::Inbox);
    assert_eq!(resolve(FolderRole::Inbox, &[inbox]), Some("INBOX"));
}
