//! `CurrencyCode` — ISO 4217 currency code value object.
//!
//! Enforces the invariant: exactly 3 uppercase ASCII letters.
//! Examples: "USD", "EUR", "MDL"

use crate::errors::DomainError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// An ISO 4217 currency code. Always exactly 3 uppercase ASCII letters.
///
/// # Examples
/// ```
/// use domain::value_objects::CurrencyCode;
///
/// let usd = CurrencyCode::new("USD").unwrap();
/// let eur = CurrencyCode::new("EUR").unwrap();
/// assert_ne!(usd, eur);
///
/// // Invalid codes are rejected
/// assert!(CurrencyCode::new("us").is_err());   // lowercase
/// assert!(CurrencyCode::new("USDD").is_err()); // too long
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CurrencyCode(String);

impl CurrencyCode {
    /// Creates a new `CurrencyCode`, validating the format.
    pub fn new(code: &str) -> Result<Self, DomainError> {
        let code = code.trim();
        if code.len() == 3 && code.chars().all(|c| c.is_ascii_uppercase()) {
            Ok(Self(code.to_owned()))
        } else {
            Err(DomainError::InvalidCurrencyCode {
                code: code.to_owned(),
            })
        }
    }

    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CurrencyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for CurrencyCode {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

impl TryFrom<&str> for CurrencyCode {
    type Error = DomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_codes_are_accepted() {
        assert!(CurrencyCode::new("USD").is_ok());
        assert!(CurrencyCode::new("EUR").is_ok());
        assert!(CurrencyCode::new("MDL").is_ok());
        assert!(CurrencyCode::new("GBP").is_ok());
    }

    #[test]
    fn lowercase_is_rejected() {
        assert!(CurrencyCode::new("usd").is_err());
        assert!(CurrencyCode::new("Usd").is_err());
    }

    #[test]
    fn wrong_length_is_rejected() {
        assert!(CurrencyCode::new("US").is_err());
        assert!(CurrencyCode::new("USDD").is_err());
        assert!(CurrencyCode::new("").is_err());
    }

    #[test]
    fn display_returns_uppercase_code() {
        let code = CurrencyCode::new("MDL").unwrap();
        assert_eq!(code.to_string(), "MDL");
    }

    #[test]
    fn equality_is_value_based() {
        let a = CurrencyCode::new("USD").unwrap();
        let b = CurrencyCode::new("USD").unwrap();
        assert_eq!(a, b);
    }
}
