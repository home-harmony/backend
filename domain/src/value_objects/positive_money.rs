//! `PositiveMoney` — strictly positive (`> 0`) currency-tagged monetary amount.
//!
//! Enforced on every ledger transaction leg (`amount` and `destination_amount`)
//! to prevent zero-value ledger noise and division-by-zero in implied FX rates.

use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    errors::DomainError,
    value_objects::{CurrencyCode, Money},
};

/// A strictly positive (`> 0`) monetary amount with an associated [`CurrencyCode`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawPositiveMoney")]
pub struct PositiveMoney {
    amount: Decimal,
    currency: CurrencyCode,
}

#[derive(Deserialize)]
struct RawPositiveMoney {
    amount: Decimal,
    currency: CurrencyCode,
}

impl PositiveMoney {
    /// Creates a new `PositiveMoney`, rejecting zero and negative amounts.
    pub fn new(amount: Decimal, currency: CurrencyCode) -> Result<Self, DomainError> {
        if amount <= Decimal::ZERO {
            return Err(DomainError::NonPositiveAmount { amount });
        }
        Ok(Self { amount, currency })
    }

    /// Returns the exact strictly positive decimal amount.
    #[must_use]
    pub fn amount(&self) -> Decimal {
        self.amount
    }

    /// Returns the ISO 4217 currency code.
    #[must_use]
    pub fn currency(&self) -> &CurrencyCode {
        &self.currency
    }

    /// Converts a borrow into a non-negative [`Money`] value.
    #[must_use]
    pub fn as_money(&self) -> Money {
        Money::from_positive(self)
    }

    /// Consumes `self` into a non-negative [`Money`] value.
    #[must_use]
    pub fn into_money(self) -> Money {
        Money::from_positive(&self)
    }
}

impl From<PositiveMoney> for Money {
    fn from(value: PositiveMoney) -> Self {
        Self::from_positive(&value)
    }
}

impl TryFrom<RawPositiveMoney> for PositiveMoney {
    type Error = DomainError;

    fn try_from(raw: RawPositiveMoney) -> Result<Self, Self::Error> {
        Self::new(raw.amount, raw.currency)
    }
}

impl fmt::Display for PositiveMoney {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.currency)
    }
}
