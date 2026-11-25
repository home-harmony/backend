//! `Money` — the core monetary value object.
//!
//! # Rules (non-negotiable per AGENTS.md & ADR-0005)
//!
//! - Amount is **always** `rust_decimal::Decimal` — never `f64` or `f32`
//! - Currency safety: arithmetic between two `Money` values requires matching currencies
//! - Amount must be strictly zero or positive (`>= 0`), enforced at construction,
//!   deserialization, subtraction, and multiplication. Signed balances belong to
//!   `SignedMoney` / `AccountBalanceSnapshot`, not `Money`.
//!
//! # Examples
//!
//! ```
//! use domain::value_objects::{CurrencyCode, Money};
//! use rust_decimal_macros::dec;
//!
//! let price = Money::new(dec!(42.50), CurrencyCode::try_new("USD").unwrap()).unwrap();
//! let tax   = Money::new(dec!(3.80),  CurrencyCode::try_new("USD").unwrap()).unwrap();
//!
//! let total = price.add(&tax).unwrap();
//! assert_eq!(total.amount(), dec!(46.30));
//! ```

use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{errors::DomainError, value_objects::CurrencyCode};

/// A monetary amount with an associated currency.
///
/// Arithmetic is currency-safe and checked against overflow and negative invariants:
/// adding USD to EUR returns `DomainError::CurrencyMismatch`, and negative or
/// deserialized negative amounts return `DomainError::NegativeAmount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawMoney")]
pub struct Money {
    /// The decimal amount. Uses `Decimal` for exact representation (no float rounding).
    amount: Decimal,
    /// ISO 4217 currency code.
    currency: CurrencyCode,
}

#[derive(Deserialize)]
struct RawMoney {
    amount: Decimal,
    currency: CurrencyCode,
}

impl Money {
    /// Creates a new `Money` value.
    ///
    /// # Errors
    /// Returns [`DomainError::NegativeAmount`] if `amount` is negative.
    pub fn new(amount: Decimal, currency: CurrencyCode) -> Result<Self, DomainError> {
        if amount < Decimal::ZERO {
            return Err(DomainError::NegativeAmount { amount });
        }
        Ok(Self { amount, currency })
    }

    /// Creates `Money` from a raw string amount and currency code string.
    /// Convenience constructor for tests and migrations.
    pub fn from_str_parts(amount: &str, currency: &str) -> Result<Self, DomainError> {
        let amount: Decimal = amount
            .parse()
            .map_err(|_| DomainError::InvariantViolation {
                message: "Invalid decimal amount format".to_string(),
            })?;
        let currency = CurrencyCode::try_new(currency)?;
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
        let amount =
            self.amount
                .checked_add(other.amount)
                .ok_or(DomainError::ArithmeticOverflow {
                    operation: "money addition",
                })?;
        Ok(Self {
            amount,
            currency: self.currency.clone(),
        })
    }

    /// Subtracts `other` from `self`. Currencies must match.
    ///
    /// # Errors
    /// Returns [`DomainError::CurrencyMismatch`] if currencies differ,
    /// [`DomainError::ArithmeticOverflow`] on decimal overflow, or
    /// [`DomainError::NegativeAmount`] if `self.amount < other.amount`.
    pub fn sub(&self, other: &Self) -> Result<Self, DomainError> {
        self.assert_same_currency(other)?;
        let amount =
            self.amount
                .checked_sub(other.amount)
                .ok_or(DomainError::ArithmeticOverflow {
                    operation: "money subtraction",
                })?;
        Self::new(amount, self.currency.clone())
    }

    /// Multiplies the amount by a non-negative scalar factor (e.g., for interest calculations).
    ///
    /// # Errors
    /// Returns [`DomainError::NegativeAmount`] if `factor < 0`, or
    /// [`DomainError::ArithmeticOverflow`] on decimal overflow.
    pub fn multiply(&self, factor: Decimal) -> Result<Self, DomainError> {
        if factor < Decimal::ZERO {
            return Err(DomainError::NegativeAmount { amount: factor });
        }
        let amount = self
            .amount
            .checked_mul(factor)
            .ok_or(DomainError::ArithmeticOverflow {
                operation: "money multiplication",
            })?;
        Self::new(amount, self.currency.clone())
    }

    fn assert_same_currency(&self, other: &Self) -> Result<(), DomainError> {
        if self.currency != other.currency {
            Err(DomainError::CurrencyMismatch {
                expected: self.currency.clone(),
                actual: other.currency.clone(),
            })
        } else {
            Ok(())
        }
    }
}

impl TryFrom<RawMoney> for Money {
    type Error = DomainError;

    fn try_from(raw: RawMoney) -> Result<Self, Self::Error> {
        Self::new(raw.amount, raw.currency)
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
        let usd = CurrencyCode::try_new("USD").unwrap();
        let money = Money::new(dec!(100.50), usd.clone()).unwrap();
        assert_eq!(money.amount(), dec!(100.50));
        assert_eq!(money.currency(), &usd);
    }

    #[test]
    fn test_money_creation_negative_fails() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let err = Money::new(dec!(-10.00), usd).unwrap_err();
        assert!(matches!(err, DomainError::NegativeAmount { .. }));
    }

    #[test]
    fn test_money_add_same_currency() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let a = Money::new(dec!(10.00), usd.clone()).unwrap();
        let b = Money::new(dec!(20.50), usd).unwrap();
        let sum = a.add(&b).unwrap();
        assert_eq!(sum.amount(), dec!(30.50));
    }

    #[test]
    fn test_money_add_different_currency_fails() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let eur = CurrencyCode::try_new("EUR").unwrap();
        let a = Money::new(dec!(10.00), usd).unwrap();
        let b = Money::new(dec!(20.50), eur).unwrap();
        let err = a.add(&b).unwrap_err();
        assert!(matches!(err, DomainError::CurrencyMismatch { .. }));
    }

    #[test]
    fn zero_is_allowed() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let m = Money::new(dec!(0), usd).unwrap();
        assert!(m.is_zero());
    }

    #[test]
    fn subtraction_valid_and_below_zero_returns_negative_amount_error() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let high = Money::new(dec!(50.00), usd.clone()).unwrap();
        let low = Money::new(dec!(20.00), usd.clone()).unwrap();
        assert_eq!(high.sub(&low).unwrap().amount(), dec!(30.00));

        let balance = Money::new(dec!(5.00), usd.clone()).unwrap();
        let charge = Money::new(dec!(10.00), usd).unwrap();
        let err = balance.sub(&charge).unwrap_err();
        assert!(matches!(err, DomainError::NegativeAmount { .. }));
    }

    #[test]
    fn multiply_by_scalar_enforces_non_negative_factor() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let principal = Money::new(dec!(1000.00), usd.clone()).unwrap();
        let with_interest = principal.multiply(dec!(1.05)).unwrap();
        assert_eq!(with_interest.amount(), dec!(1050.00));

        let zero_scaled = Money::zero(usd).multiply(dec!(-1)).unwrap_err();
        assert!(matches!(zero_scaled, DomainError::NegativeAmount { .. }));

        let neg_scaled = principal.multiply(dec!(-0.5)).unwrap_err();
        assert!(matches!(neg_scaled, DomainError::NegativeAmount { .. }));
    }

    #[test]
    fn arithmetic_overflow_returns_domain_error() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let max_money = Money::new(Decimal::MAX, usd.clone()).unwrap();
        let one = Money::new(dec!(1), usd).unwrap();

        let add_err = max_money.add(&one).unwrap_err();
        assert!(matches!(
            add_err,
            DomainError::ArithmeticOverflow {
                operation: "money addition"
            }
        ));

        let mul_err = max_money.multiply(dec!(2)).unwrap_err();
        assert!(matches!(
            mul_err,
            DomainError::ArithmeticOverflow {
                operation: "money multiplication"
            }
        ));
    }

    #[test]
    fn serde_round_trips_and_rejects_negative_or_invalid_currency() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let money = Money::new(dec!(42.50), usd).unwrap();
        let json = serde_json::to_string(&money).unwrap();
        assert_eq!(json, r#"{"amount":"42.50","currency":"USD"}"#);

        let parsed: Money = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, money);

        assert!(serde_json::from_str::<Money>(r#"{"amount":"-10.00","currency":"USD"}"#).is_err());
        assert!(serde_json::from_str::<Money>(r#"{"amount":"10.00","currency":"usd"}"#).is_err());
    }

    #[test]
    fn display_format() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let m = Money::new(dec!(42.50), usd).unwrap();
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
        let m = Money::zero(CurrencyCode::try_new("EUR").unwrap());
        assert!(m.is_zero());
        assert_eq!(m.currency().as_str(), "EUR");
    }
}
