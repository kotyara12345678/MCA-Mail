/// A parsed RFC 5322 internet address (`From`, `To`, `Cc` entries).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EmailAddress {
    /// Optional display name, e.g. `Иван Петров`.
    pub name: Option<String>,
    /// Lowercased addr-spec, e.g. `ivan@example.com`.
    pub address: String,
}

impl EmailAddress {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            name: None,
            address: address.into().trim().to_ascii_lowercase(),
        }
    }

    pub fn with_name(address: impl Into<String>, name: Option<String>) -> Self {
        Self {
            name,
            address: address.into().trim().to_ascii_lowercase(),
        }
    }

    /// `Name <address>` when a display name exists, otherwise bare address.
    pub fn display(&self) -> String {
        match self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            Some(name) => format!("{name} <{}>", self.address),
            None => self.address.clone(),
        }
    }

    /// Local part of the address, used to detect role accounts and newsletters.
    pub fn local_part(&self) -> &str {
        self.address.split('@').next().unwrap_or_default()
    }

    /// Domain part of the address, lowercased.
    pub fn domain(&self) -> &str {
        self.address.rsplit('@').next().unwrap_or_default()
    }
}

impl std::fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display())
    }
}

/// Normalized message subject: reply prefixes removed and whitespace collapsed.
///
/// Classification relies on the same value the agent sees, so normalization
/// happens once at the boundary instead of in every consumer.
pub fn normalize_subject(subject: &str) -> String {
    const MAX_PREFIXES: usize = 5;
    let mut value = subject.trim();
    for _ in 0..MAX_PREFIXES {
        // `fwd:` and its common localized spellings all reduce to the same
        // prefix, so a forwarded request threads with the original.
        let lower = value.to_ascii_lowercase();
        let stripped = [
            "re:",
            "fw:",
            "fwd:",
            "ответ:",
            "пересылка:",
            // Localised reply prefixes: a customer answering in Chinese or
            // German must land in the same thread, not spawn a new lead.
            "回复:",
            "回复：",
            "答复:",
            "答复：",
            "转发:",
            "转发：",
            "aw:",
            "wg:",
            "vs:",
            "sv:",
            "tr:",
            "iv:",
        ]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix));
        match stripped {
            Some(rest) => {
                // Advance by the byte length of the *matched* prefix, not the
                // ASCII-lowercased copy: some prefixes are multi-byte in UTF-8.
                let cut = lower.len() - rest.len();
                value = value[cut..].trim_start();
            }
            None => break,
        }
    }
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_subject_strips_nested_replies() {
        assert_eq!(normalize_subject("Re: FWD: Re:  Quote "), "Quote");
        assert_eq!(normalize_subject("  Hello   world "), "Hello world");
        assert_eq!(normalize_subject(""), "");
    }

    /// Localised reply prefixes must reduce to the same thread key, or a
    /// Chinese or German customer's answer spawns a fresh lead.
    #[test]
    fn normalize_subject_strips_localised_reply_prefixes() {
        assert_eq!(normalize_subject("回复：Re: Тарифы"), "Тарифы");
        assert_eq!(normalize_subject("回复: Тарифы"), "Тарифы");
        assert_eq!(normalize_subject("答复： Тарифы"), "Тарифы");
        assert_eq!(normalize_subject("转发: Тарифы"), "Тарифы");
        assert_eq!(normalize_subject("AW: Тарифы"), "Тарифы");
        assert_eq!(normalize_subject("vs: Тарифы"), "Тарифы");
        assert_eq!(normalize_subject("AW: Re: FW: Тарифы"), "Тарифы");
    }

    #[test]
    fn address_display_and_parts() {
        let a = EmailAddress::with_name("Ivan@Example.COM", Some("Иван".into()));
        assert_eq!(a.address, "ivan@example.com");
        assert_eq!(a.display(), "Иван <ivan@example.com>");
        assert_eq!(a.local_part(), "ivan");
        assert_eq!(a.domain(), "example.com");
    }

    #[test]
    fn address_without_name_is_bare() {
        assert_eq!(EmailAddress::new("a@b.c").display(), "a@b.c");
    }
}
