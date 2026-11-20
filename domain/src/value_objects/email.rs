//! `Email` — validated and normalized email address value object.
//!
//! Backed by [`nutype`]: sanitization and validation run at construction and
//! deserialization time so it is impossible to hold a blank or malformed `Email`.
//!
//! # Invariants
//! - Leading/trailing whitespace is trimmed and ASCII/Unicode characters are lowercased.
//! - Must not be empty and must be at most 254 characters (RFC 5321).
//! - Must contain exactly one `@`, a valid local part (`1..=64` chars, no leading/trailing/consecutive dots),
//!   and a valid multi-label domain with a valid TLD.
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

use nutype::nutype;

#[nutype(
    sanitize(trim, lowercase),
    validate(not_empty, len_char_max = 254, predicate = is_valid_email),
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

fn is_valid_email(value: &str) -> bool {
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    if domain.contains('@') {
        return false;
    }
    is_valid_local_part(local) && is_valid_domain(domain)
}

fn is_valid_local_part(local: &str) -> bool {
    !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
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
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
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
    }

    #[test]
    fn email_is_trimmed_and_lowercased() {
        let email = Email::try_new("  Alice.Smith@Example.COM  ").unwrap();
        assert_eq!(email.as_ref(), "alice.smith@example.com");
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
        let local_65 = format!("{}@example.com", "a".repeat(65));
        assert!(Email::try_new(local_65).is_err());
    }

    #[test]
    fn invalid_domain_is_rejected() {
        assert!(Email::try_new("alice@localhost").is_err());
        assert!(Email::try_new("alice@-example.com").is_err());
        assert!(Email::try_new("alice@example-.com").is_err());
        assert!(Email::try_new("alice@example..com").is_err());
        assert!(Email::try_new("alice@example.c").is_err());
        assert!(Email::try_new("alice@example.123").is_err());
        assert!(Email::try_new("alice@example.xn--").is_err());
    }

    #[test]
    fn over_254_chars_is_rejected() {
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
    }

    #[test]
    fn serde_normalizes_and_validates() {
        let email: Email = serde_json::from_str("\"  Alice@Example.COM \"").unwrap();
        assert_eq!(email.as_ref(), "alice@example.com");
        assert_eq!(
            serde_json::to_string(&email).unwrap(),
            "\"alice@example.com\""
        );

        assert!(serde_json::from_str::<Email>("\"not-an-email\"").is_err());
        assert!(serde_json::from_str::<Email>("\"\"").is_err());
    }
}
