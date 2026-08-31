//! `Money` — the core monetary value object.
//!
//! # Rules (non-negotiable per GEMINI.md)
//!
//! - Amount is **always** `rust_decimal::Decimal` — never `f64` or `f32`
//! - Currency safety: arithmetic between two `Money` values requires matching currencies
//! - Amount must be non-negative for any monetary value (use signed amounts only where
//!   the domain explicitly models debt/credit, e.g., account balance)
//!
//! # Examples
//!
//! ```
//! use domain::value_objects::{Money, CurrencyCode};
//! use rust_decimal_macros::dec;
//!
//! let price = Money::new(dec!(42.50), CurrencyCode::new("USD").unwrap()).unwrap();
//! let tax   = Money::new(dec!(3.80),  CurrencyCode::new("USD").unwrap()).unwrap();
//!
//! let total = price.add(&tax).unwrap();
//! assert_eq!(total.amount(), dec!(46.30));
//! ```

use crate::{errors::DomainError, value_objects::CurrencyCode};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A monetary amount with an associated currency.
///
/// Arithmetic is currency-safe: adding USD to EUR returns `DomainError::CurrencyMismatch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    /// The decimal amount. Uses `Decimal` for exact representation (no float rounding).
    amount: Decimal,
    /// ISO 4217 currency code.
    currency: CurrencyCode,
}

impl Money {
    /// Creates a new `Money` value.
    ///
    /// # Errors
    /// Returns [`DomainError::NegativeAmount`] if `amount` is negative.
    pub fn new(amount: Decimal, currency: CurrencyCode) -> Result<Self, DomainError> {
        if amount < Decimal::ZERO {
            return Err(DomainError::NegativeAmount {
                amount: amount.to_string(),
            });
        }
        Ok(Self { amount, currency })
    }

    /// Creates `Money` from a raw string amount and currency code string.
    /// Convenience constructor for tests and migrations.
    pub fn from_str_parts(amount: &str, currency: &str) -> Result<Self, DomainError> {
        let amount: Decimal = amount
            .parse()
            .map_err(|_| DomainError::InvariantViolation {
                message: format!("Cannot parse '{}' as Decimal", amount),
            })?;
        let currency = CurrencyCode::new(currency)?;
        Self::new(amount, currency)
    }

    /// Returns a zero-value `Money` in the given currency.
    pub fn zero(currency: CurrencyCode) -> Self {
        Self {
            amount: Decimal::ZERO,
            currency,
        }
    }

    /// The decimal amount.
    pub fn amount(&self) -> Decimal {
        self.amount
    }

    /// The ISO 4217 currency code.
    pub fn currency(&self) -> &CurrencyCode {
        &self.currency
    }

    /// Returns `true` if the amount is exactly zero.
    pub fn is_zero(&self) -> bool {
        self.amount == Decimal::ZERO
    }

    /// Adds two `Money` values. Currencies must match.
    pub fn add(&self, other: &Self) -> Result<Self, DomainError> {
        self.assert_same_currency(other)?;
        Ok(Self {
            amount: self.amount + other.amount,
            currency: self.currency.clone(),
        })
    }

    /// Subtracts `other` from `self`. Currencies must match.
    /// Result may be negative (e.g., a credit card account balance can go negative).
    pub fn sub(&self, other: &Self) -> Result<Self, DomainError> {
        self.assert_same_currency(other)?;
        // Note: we allow negative results here since account balances can be negative
        let amount = self.amount - other.amount;
        Ok(Self {
            amount,
            currency: self.currency.clone(),
        })
    }

    /// Multiplies the amount by a scalar factor (e.g., for interest calculations).
    pub fn multiply(&self, factor: Decimal) -> Self {
        Self {
            amount: self.amount * factor,
            currency: self.currency.clone(),
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────────

    fn assert_same_currency(&self, other: &Self) -> Result<(), DomainError> {
        if self.currency != other.currency {
            Err(DomainError::CurrencyMismatch {
                expected: self.currency.to_string(),
                actual: other.currency.to_string(),
            })
        } else {
            Ok(())
        }
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.currency)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_money_creation_valid() {
        let usd = CurrencyCode::new("USD").unwrap();
        let money = Money::new(dec!(100.50), usd.clone()).unwrap();
        assert_eq!(money.amount(), dec!(100.50));
        assert_eq!(money.currency(), &usd);
    }

    #[test]
    fn test_money_creation_negative_fails() {
        let usd = CurrencyCode::new("USD").unwrap();
        let err = Money::new(dec!(-10.00), usd).unwrap_err();
        assert!(matches!(err, DomainError::NegativeAmount { .. }));
    }

    #[test]
    fn test_money_add_same_currency() {
        let usd = CurrencyCode::new("USD").unwrap();
        let a = Money::new(dec!(10.00), usd.clone()).unwrap();
        let b = Money::new(dec!(20.50), usd).unwrap();
        let sum = a.add(&b).unwrap();
        assert_eq!(sum.amount(), dec!(30.50));
    }

    #[test]
    fn test_money_add_different_currency_fails() {
        let usd = CurrencyCode::new("USD").unwrap();
        let eur = CurrencyCode::new("EUR").unwrap();
        let a = Money::new(dec!(10.00), usd).unwrap();
        let b = Money::new(dec!(20.50), eur).unwrap();
        let err = a.add(&b).unwrap_err();
        assert!(matches!(err, DomainError::CurrencyMismatch { .. }));
    }
}
