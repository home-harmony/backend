//! `Email` — validated and normalized email address value object.
//!
//! Backed by [`nutype`] and [`email_address`]: sanitization and validation run at
//! construction and deserialization time so it is impossible to hold a blank or
//! malformed `Email`.
//!
//! # Invariants
//! - Leading/trailing whitespace is trimmed and characters are lowercased.
//! - Must not be empty and must be at most 254 UTF-8 bytes (RFC 5321).
//! - Must parse cleanly via `email_address` with a required TLD (`>= 2` domain labels),
//!   without display names (`Name <...>`), domain literals (`[...]`), or quoted local parts.
//! - Domain labels must be valid DNS/IDN labels (`1..=63` chars, no leading/trailing hyphens)
//!   ending in a valid TLD (`>= 2` ASCII letters or punycode `xn--...`).
//!
//! # Examples
//! ```
//! use domain::value_objects::Email;
//!
//! let email = Email::try_new("  Alice@Example.COM  ").unwrap();
//! assert_eq!(email.into_inner(), "alice@example.com");
//!
//! assert!(Email::try_new("").is_err());
//! assert!(Email::try_new("not-an-email").is_err());
//! ```

use email_address::{EmailAddress, Options};
use nutype::nutype;

#[nutype(
    sanitize(trim, lowercase),
    validate(not_empty, predicate = is_valid_email),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        AsRef,
        Display,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct Email(String);

impl Email {
    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

fn is_valid_email(value: &str) -> bool {
    if value.len() > 254 || value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let options = Options::default()
        .with_required_tld()
        .without_domain_literal()
        .without_display_text();
    let Ok(parsed) = EmailAddress::parse_with_options(value, options) else {
        return false;
    };
    if parsed.local_part().starts_with('"') {
        return false;
    }
    is_valid_domain(parsed.domain())
}

fn is_valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 || !domain.contains('.') {
        return false;
    }
    let mut last_label = "";
    for label in domain.split('.') {
        if !is_valid_domain_label(label) {
            return false;
        }
        last_label = label;
    }
    is_valid_tld(last_label)
}

fn is_valid_domain_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '-' || (!c.is_ascii() && c.is_alphanumeric())
        })
}

fn is_valid_tld(tld: &str) -> bool {
    tld.len() >= 2
        && (tld.bytes().all(|b| b.is_ascii_alphabetic())
            || (tld.starts_with("xn--") && tld.len() > 4))
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_emails_are_accepted() {
        assert!(Email::try_new("alice@example.com").is_ok());
        assert!(Email::try_new("alice+tag@sub.example.co.uk").is_ok());
        assert!(Email::try_new("first.last@my-domain.xn--p1ai").is_ok());
        assert!(Email::try_new("пользователь@example.com").is_ok());
        assert!(Email::try_new("Dörte@Sörensen.example.com").is_ok());
    }

    #[test]
    fn email_is_trimmed_and_lowercased() {
        let email = Email::try_new("  Alice.Smith@Example.COM  ").unwrap();
        assert_eq!(email.as_str(), "alice.smith@example.com");
        assert_eq!(email.to_string(), "alice.smith@example.com");
    }

    #[test]
    fn empty_or_whitespace_is_rejected() {
        assert!(Email::try_new("").is_err());
        assert!(Email::try_new("   ").is_err());
    }

    #[test]
    fn invalid_at_usage_is_rejected() {
        assert!(Email::try_new("no-at-sign.example.com").is_err());
        assert!(Email::try_new("a@b@example.com").is_err());
        assert!(Email::try_new("@example.com").is_err());
        assert!(Email::try_new("alice@").is_err());
    }

    #[test]
    fn invalid_local_part_is_rejected() {
        assert!(Email::try_new(".alice@example.com").is_err());
        assert!(Email::try_new("alice.@example.com").is_err());
        assert!(Email::try_new("al..ice@example.com").is_err());
        assert!(Email::try_new("al ice@example.com").is_err());
        assert!(Email::try_new("<script>@example.com").is_err());
        assert!(Email::try_new("\"quoted\"@example.com").is_err());
        assert!(Email::try_new("Alice <alice@example.com>").is_err());
        let local_65 = format!("{}@example.com", "a".repeat(65));
        assert!(Email::try_new(local_65).is_err());
    }

    #[test]
    fn invalid_domain_is_rejected() {
        assert!(Email::try_new("alice@localhost").is_err());
        assert!(Email::try_new("alice@[127.0.0.1]").is_err());
        assert!(Email::try_new("alice@-example.com").is_err());
        assert!(Email::try_new("alice@example-.com").is_err());
        assert!(Email::try_new("alice@example..com").is_err());
        assert!(Email::try_new("alice@ex_ample.com").is_err());
        assert!(Email::try_new("alice@ex!ample.com").is_err());
        assert!(Email::try_new("alice@example.c").is_err());
        assert!(Email::try_new("alice@example.123").is_err());
        assert!(Email::try_new("alice@example.xn--").is_err());
    }

    #[test]
    fn over_254_bytes_is_rejected() {
        let domain = format!(
            "{}.{}.{}.com",
            "a".repeat(61),
            "b".repeat(60),
            "c".repeat(63)
        );
        assert_eq!(domain.len(), 190);

        let at_limit = format!("{}@{}", "u".repeat(63), domain);
        assert_eq!(at_limit.len(), 254);
        assert!(Email::try_new(at_limit).is_ok());

        let over_limit = format!("{}@{}", "u".repeat(64), domain);
        assert_eq!(over_limit.len(), 255);
        assert!(Email::try_new(over_limit).is_err());

        let multibyte_over_limit = format!("{}@example.com", "é".repeat(122));
        assert!(multibyte_over_limit.chars().count() < 254);
        assert!(multibyte_over_limit.len() > 254);
        assert!(Email::try_new(multibyte_over_limit).is_err());
    }

    #[test]
    fn serde_normalizes_and_validates() {
        let email: Email = serde_json::from_str("\"  Alice@Example.COM \"").unwrap();
        assert_eq!(email.as_str(), "alice@example.com");
        assert_eq!(
            serde_json::to_string(&email).unwrap(),
            "\"alice@example.com\""
        );

        assert!(serde_json::from_str::<Email>("\"not-an-email\"").is_err());
        assert!(serde_json::from_str::<Email>("\"\"").is_err());
    }
}
