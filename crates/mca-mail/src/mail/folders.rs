use async_imap::types::NameAttribute;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRole {
    Inbox,
    Archive,
    Drafts,
    Sent,
    Spam,
    Trash,
    Other,
}

impl FolderRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Archive => "archive",
            Self::Drafts => "drafts",
            Self::Sent => "sent",
            Self::Spam => "spam",
            Self::Trash => "trash",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInfo {
    pub name: String,
    pub role: FolderRole,
    pub special_use: Vec<String>,
    pub selectable: bool,
}

impl FolderInfo {
    pub fn from_list(name: &str, attributes: &[NameAttribute<'_>]) -> Self {
        let mut roles = Vec::new();
        let mut special_use = Vec::new();
        for attribute in attributes {
            let role = match attribute {
                NameAttribute::Archive => Some(FolderRole::Archive),
                NameAttribute::Drafts => Some(FolderRole::Drafts),
                NameAttribute::Sent => Some(FolderRole::Sent),
                NameAttribute::Junk => Some(FolderRole::Spam),
                NameAttribute::Trash => Some(FolderRole::Trash),
                _ => None,
            };
            let special = match attribute {
                NameAttribute::All => Some("\\All".to_string()),
                NameAttribute::Archive => Some("\\Archive".to_string()),
                NameAttribute::Drafts => Some("\\Drafts".to_string()),
                NameAttribute::Flagged => Some("\\Flagged".to_string()),
                NameAttribute::Junk => Some("\\Junk".to_string()),
                NameAttribute::Sent => Some("\\Sent".to_string()),
                NameAttribute::Trash => Some("\\Trash".to_string()),
                _ => None,
            };
            if let Some(flag) = special {
                special_use.push(flag);
            }
            if let Some(role) = role {
                roles.push(role);
            }
        }
        let role = if name.eq_ignore_ascii_case("INBOX") {
            FolderRole::Inbox
        } else {
            match roles.as_slice() {
                [role] => *role,
                _ => FolderRole::Other,
            }
        };
        let selectable = !attributes
            .iter()
            .any(|attribute| matches!(attribute, NameAttribute::NoSelect));
        Self {
            name: name.into(),
            role,
            special_use,
            selectable,
        }
    }
}

pub fn resolve(role: FolderRole, folders: &[FolderInfo]) -> Option<&str> {
    let mut matches = folders
        .iter()
        .filter(|folder| folder.role == role && role != FolderRole::Other);
    let folder = matches.next()?;
    if !folder.selectable || matches.next().is_some() {
        return None;
    }
    Some(folder.name.as_str())
}

#[cfg(test)]
#[path = "folders_test.rs"]
mod tests;
