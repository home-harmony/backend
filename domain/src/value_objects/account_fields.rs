//! Validated value objects for `PaymentAccount` attributes (`VO-15`, `VO-16`).

use nutype::nutype;

/// Validated payment account name (`1..=200` trimmed Unicode chars, control/BiDi safe).
#[nutype(
    sanitize(trim),
    validate(
        not_empty,
        len_char_max = 200,
        predicate = crate::value_objects::is_safe_display_text
    ),
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
pub struct AccountName(String);

impl AccountName {
    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

/// Validated issuing bank or institution name (`1..=100` trimmed Unicode chars, control/BiDi safe).
#[nutype(
    sanitize(trim),
    validate(
        not_empty,
        len_char_max = 100,
        predicate = crate::value_objects::is_safe_display_text
    ),
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
pub struct BankName(String);

impl BankName {
    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

/// Validated last 4 digits of a payment card or bank account (`^\d{4}$`).
#[nutype(
    sanitize(trim),
    validate(predicate = is_four_ascii_digits),
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
pub struct LastFour(String);

impl LastFour {
    /// Returns the inner 4-digit ASCII string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

fn is_four_ascii_digits(value: &str) -> bool {
    value.len() == 4 && value.bytes().all(|b| b.is_ascii_digit())
}
