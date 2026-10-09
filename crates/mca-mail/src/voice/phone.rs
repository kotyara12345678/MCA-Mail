//! Phone-number handling for the voice channel.
//!
//! A number may reach us in any human format — `+7 916 123-45-67`, `89161234567`,
//! `7-916-123-45-67`. Only the digits matter: they form the stable identity a
//! voice lead is keyed on, so the same caller always resumes the same lead.

use crate::error::AppError;

/// Strip everything except ASCII digits.
fn digits(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Canonicalize a digit string: the Russian trunk prefix `8` and the
/// international `7` are the same subscriber, and a caller who dials one form
/// today and the other tomorrow must land on the same lead.
///
/// Only the exact 11-digit domestic shape is rewritten; every other length is
/// left alone, so international numbers are never guessed at.
fn canonicalize(number: String) -> String {
    if number.len() == 11 && number.starts_with('8') {
        let mut rewritten = String::with_capacity(11);
        rewritten.push('7');
        rewritten.push_str(&number[1..]);
        rewritten
    } else {
        number
    }
}

/// Normalize a caller number to a plain digit string of plausible length.
///
/// Accepts 7–15 digits (the E.164 range, without forcing the `+`). Anything
/// outside that range is an error: it cannot be a real phone number.
pub fn normalize_phone(raw: &str) -> Result<String, AppError> {
    let cleaned = canonicalize(digits(raw));
    if !(7..=15).contains(&cleaned.len()) {
        return Err(AppError::invalid(format!(
            "caller number `{raw}` does not look like a phone number"
        )));
    }
    Ok(cleaned)
}

/// The idempotent conversation key for a voice lead: `voice:<digits>`.
///
/// Distinct from the email keys (`address+subject`) by prefix, so a number can
/// never collide with a mail thread, and from `manual:...` keys so the same
/// caller reaching us twice still maps to one lead.
pub fn voice_key(raw: &str) -> String {
    format!("voice:{}", canonicalize(digits(raw)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_formatting() {
        assert_eq!(
            normalize_phone("+7 (916) 123-45-67").unwrap(),
            "79161234567"
        );
    }

    #[test]
    fn voice_key_is_digits_only() {
        assert_eq!(voice_key("+7 916 123 45 67"), "voice:79161234567");
    }

    #[test]
    fn trunk_eight_and_seven_are_the_same_subscriber() {
        // Continuation of one lead: the same human dialled in two formats.
        assert_eq!(voice_key("89161234567"), voice_key("+79161234567"));
        assert_eq!(normalize_phone("8 916 123-45-67").unwrap(), "79161234567");
        // An 11-digit number that does not start with 8 is left alone.
        assert_eq!(normalize_phone("79161234567").unwrap(), "79161234567");
        // Other lengths are never rewritten: this is a 7-digit local shorty.
        assert_eq!(normalize_phone("8123456").unwrap(), "8123456");
        // International numbers starting with 8 keep their shape.
        assert_eq!(normalize_phone("+8613800138000").unwrap(), "8613800138000");
    }

    #[test]
    fn too_short_rejected() {
        assert!(normalize_phone("123").is_err());
        assert!(normalize_phone("").is_err());
    }

    #[test]
    fn long_international_ok() {
        assert_eq!(normalize_phone("+8613800138000").unwrap(), "8613800138000");
    }
}
