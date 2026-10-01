use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroize;

/// A string that must never appear in logs, `Debug` output or error messages.
///
/// Credentials and API keys are stored in this type so that an accidental
/// `{:?}` in a log statement cannot leak them: the `Debug` implementation prints
/// a redaction marker and the value is zeroed on drop.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn empty() -> Self {
        Self(String::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }

    /// Reveal the value. Named to make every call site auditable.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Reveal into an owned `String` for clients that need ownership.
    pub fn expose_owned(&self) -> String {
        self.0.clone()
    }

    /// `true` when the value is present and looks usable.
    pub fn is_present(&self) -> bool {
        !self.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            f.write_str("Secret(<empty>)")
        } else {
            f.write_str("Secret(<redacted>)")
        }
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Best-effort scrubbing: the allocator may hold earlier copies.
        self.0.zeroize();
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Option::<String>::deserialize(deserializer)?;
        Ok(Secret(value.unwrap_or_default()))
    }
}

impl Serialize for Secret {
    /// Serializes as `null` so an API response can never echo a secret back.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_value() {
        let s = Secret::new("hunter2-super-secret");
        let rendered = format!("{s:?} {s}");
        assert!(!rendered.contains("hunter2"));
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn empty_secret_reports_absent() {
        assert!(Secret::empty().is_empty());
        assert!(!Secret::empty().is_present());
        assert!(Secret::new("x").is_present());
    }

    #[test]
    fn serializes_as_null() {
        let s = Secret::new("abc");
        assert_eq!(serde_json::to_string(&s).unwrap(), "null");
    }
}
