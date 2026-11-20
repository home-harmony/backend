//! `CurrencyCode` — ISO 4217 currency code value object.
//!
//! Backed by [`nutype`]: validation runs at construction and deserialization time,
//! enforcing the invariant of exactly 3 uppercase ASCII letters.
//! Examples: "USD", "EUR", "MDL"

use nutype::nutype;

use crate::errors::DomainError;

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
#[nutype(
    sanitize(trim),
    validate(predicate = is_valid_currency_code),
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
pub struct CurrencyCode(String);

impl CurrencyCode {
    /// Creates a new `CurrencyCode`, validating the format.
    pub fn new(code: &str) -> Result<Self, DomainError> {
        Self::try_new(code).map_err(|_| DomainError::InvalidCurrencyCode {
            code: code.trim().to_owned(),
        })
    }

    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
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

fn is_valid_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
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

    #[test]
    fn serde_validates_on_deserialization() {
        let usd: CurrencyCode = serde_json::from_str("\"USD\"").unwrap();
        assert_eq!(usd.as_str(), "USD");

        let trimmed: CurrencyCode = serde_json::from_str("\" MDL \"").unwrap();
        assert_eq!(trimmed.as_str(), "MDL");

        assert!(serde_json::from_str::<CurrencyCode>("\"\"").is_err());
        assert!(serde_json::from_str::<CurrencyCode>("\"usd\"").is_err());
        assert!(serde_json::from_str::<CurrencyCode>("\"US\"").is_err());
        assert!(serde_json::from_str::<CurrencyCode>("\"USDD\"").is_err());
    }
}
