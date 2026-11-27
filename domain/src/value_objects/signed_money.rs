//! `SignedMoney` — float-free currency-tagged signed monetary value (`ADR-0005`, `ADR-0006`).
//!
//! Used for derived account balances, month-end `AccountBalanceSnapshot`s,
//! `PaymentAccount.opening_balance`, and `FXSpread` calculations where negative,
//! zero, and positive values are all domain-valid.

use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    errors::DomainError,
    value_objects::{CurrencyCode, Money},
};

/// A signed monetary amount with an associated [`CurrencyCode`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawSignedMoney")]
pub struct SignedMoney {
    amount: Decimal,
    currency: CurrencyCode,
}

#[derive(Deserialize)]
struct RawSignedMoney {
    amount: Decimal,
    currency: CurrencyCode,
}

impl SignedMoney {
    /// Creates a new `SignedMoney` from any `Decimal` (negative, zero, or positive) and currency.
    #[must_use]
    pub fn new(amount: Decimal, currency: CurrencyCode) -> Self {
        Self { amount, currency }
    }

    /// Creates a zero-value `SignedMoney` in the given currency.
    #[must_use]
    pub fn zero(currency: CurrencyCode) -> Self {
        Self {
            amount: Decimal::ZERO,
            currency,
        }
    }

    /// Promotes a non-negative [`Money`] into `SignedMoney`.
    #[must_use]
    pub fn from_money(money: &Money) -> Self {
        Self {
            amount: money.amount(),
            currency: money.currency().clone(),
        }
    }

    /// Returns the exact decimal amount.
    #[must_use]
    pub fn amount(&self) -> Decimal {
        self.amount
    }

    /// Returns the ISO 4217 currency code.
    #[must_use]
    pub fn currency(&self) -> &CurrencyCode {
        &self.currency
    }

    /// Returns `true` when the amount equals zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.amount == Decimal::ZERO
    }

    /// Returns `true` when the amount is strictly negative.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.amount < Decimal::ZERO
    }

    /// Returns `true` when the amount is strictly positive.
    #[must_use]
    pub fn is_positive(&self) -> bool {
        self.amount > Decimal::ZERO
    }

    /// Adds two `SignedMoney` values, enforcing matching currencies and overflow checks.
    pub fn add(&self, other: &Self) -> Result<Self, DomainError> {
        self.assert_same_currency(other)?;
        let amount =
            self.amount
                .checked_add(other.amount)
                .ok_or(DomainError::ArithmeticOverflow {
                    operation: "signed money addition",
                })?;
        Ok(Self {
            amount,
            currency: self.currency.clone(),
        })
    }

    /// Subtracts `other` from `self`, enforcing matching currencies and overflow checks.
    pub fn sub(&self, other: &Self) -> Result<Self, DomainError> {
        self.assert_same_currency(other)?;
        let amount =
            self.amount
                .checked_sub(other.amount)
                .ok_or(DomainError::ArithmeticOverflow {
                    operation: "signed money subtraction",
                })?;
        Ok(Self {
            amount,
            currency: self.currency.clone(),
        })
    }

    /// Negates this `SignedMoney` amount (`-amount`).
    pub fn negate(&self) -> Result<Self, DomainError> {
        let amount = self
            .amount
            .checked_mul(Decimal::NEGATIVE_ONE)
            .ok_or(DomainError::ArithmeticOverflow {
                operation: "signed money negation",
            })?;
        Ok(Self {
            amount,
            currency: self.currency.clone(),
        })
    }

    fn assert_same_currency(&self, other: &Self) -> Result<(), DomainError> {
        if self.currency != other.currency {
            return Err(DomainError::CurrencyMismatch {
                expected: self.currency.clone(),
                actual: other.currency.clone(),
            });
        }
        Ok(())
    }
}

impl TryFrom<RawSignedMoney> for SignedMoney {
    type Error = DomainError;

    fn try_from(raw: RawSignedMoney) -> Result<Self, Self::Error> {
        Ok(Self::new(raw.amount, raw.currency))
    }
}

impl fmt::Display for SignedMoney {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.currency)
    }
}
