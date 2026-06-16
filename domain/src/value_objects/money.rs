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
//! let total = (price + tax).unwrap();
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
        let amount: Decimal = amount.parse().map_err(|_| DomainError::InvariantViolation {
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

    // ── Internal helpers ──────────────────────────────────────────────────────

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

    fn usd(amount: Decimal) -> Money {
        Money::new(amount, CurrencyCode::new("USD").unwrap()).unwrap()
    }

    fn eur(amount: Decimal) -> Money {
        Money::new(amount, CurrencyCode::new("EUR").unwrap()).unwrap()
    }

    #[test]
    fn negative_amount_is_rejected() {
        let result = Money::new(dec!(-1.00), CurrencyCode::new("USD").unwrap());
        assert!(matches!(result, Err(DomainError::NegativeAmount { .. })));
    }

    #[test]
    fn zero_is_allowed() {
        let m = usd(dec!(0));
        assert!(m.is_zero());
    }

    #[test]
    fn same_currency_addition() {
        let a = usd(dec!(10.50));
        let b = usd(dec!(5.25));
        let total = a.add(&b).unwrap();
        assert_eq!(total.amount(), dec!(15.75));
    }

    #[test]
    fn cross_currency_addition_fails() {
        let a = usd(dec!(10.00));
        let b = eur(dec!(10.00));
        assert!(matches!(
            a.add(&b),
            Err(DomainError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn subtraction_can_yield_negative_balance() {
        let balance = usd(dec!(5.00));
        let charge = usd(dec!(10.00));
        let result = balance.sub(&charge).unwrap();
        // Credit card balance can go negative
        assert_eq!(result.amount(), dec!(-5.00));
    }

    #[test]
    fn multiply_by_scalar() {
        let principal = usd(dec!(1000.00));
        let with_interest = principal.multiply(dec!(1.05));
        assert_eq!(with_interest.amount(), dec!(1050.00));
    }

    #[test]
    fn display_format() {
        let m = usd(dec!(42.50));
        assert_eq!(m.to_string(), "42.50 USD");
    }

    #[test]
    fn from_str_parts() {
        let m = Money::from_str_parts("123.45", "MDL").unwrap();
        assert_eq!(m.amount(), dec!(123.45));
        assert_eq!(m.currency().as_str(), "MDL");
    }

    #[test]
    fn zero_constructor() {
        let m = Money::zero(CurrencyCode::new("EUR").unwrap());
        assert!(m.is_zero());
        assert_eq!(m.currency().as_str(), "EUR");
    }
}
