//! Domain classification enums for payment accounts, categories, and ledger transactions.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::errors::DomainError;

/// Classification of a [`PaymentAccount`](crate::entities::PaymentAccount) instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    CreditCard,
    DebitCard,
    Cash,
    BankAccount,
}

/// Classification of a ledger category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CategoryKind {
    Income,
    Expense,
    Transfer,
}

/// Classification of an immutable ledger transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionKind {
    Income,
    Expense,
    Transfer,
    CashWithdrawal,
    LoanPayment,
    Reversal,
}

impl AccountKind {
    /// Returns the canonical snake_case persistence/wire identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreditCard => "credit_card",
            Self::DebitCard => "debit_card",
            Self::Cash => "cash",
            Self::BankAccount => "bank_account",
        }
    }
}

impl fmt::Display for AccountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for AccountKind {
    type Error = DomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "credit_card" => Ok(Self::CreditCard),
            "debit_card" => Ok(Self::DebitCard),
            "cash" => Ok(Self::Cash),
            "bank_account" => Ok(Self::BankAccount),
            _ => Err(DomainError::InvariantViolation {
                message: "Unknown account kind".to_owned(),
            }),
        }
    }
}

impl TryFrom<String> for AccountKind {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl CategoryKind {
    /// Returns the canonical lowercase persistence/wire identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Income => "income",
            Self::Expense => "expense",
            Self::Transfer => "transfer",
        }
    }
}

impl fmt::Display for CategoryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for CategoryKind {
    type Error = DomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "income" => Ok(Self::Income),
            "expense" => Ok(Self::Expense),
            "transfer" => Ok(Self::Transfer),
            _ => Err(DomainError::InvariantViolation {
                message: "Unknown category kind".to_owned(),
            }),
        }
    }
}

impl TryFrom<String> for CategoryKind {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl TransactionKind {
    /// Returns the canonical snake_case persistence/wire identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Income => "income",
            Self::Expense => "expense",
            Self::Transfer => "transfer",
            Self::CashWithdrawal => "cash_withdrawal",
            Self::LoanPayment => "loan_payment",
            Self::Reversal => "reversal",
        }
    }
}

impl fmt::Display for TransactionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for TransactionKind {
    type Error = DomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "income" => Ok(Self::Income),
            "expense" => Ok(Self::Expense),
            "transfer" => Ok(Self::Transfer),
            "cash_withdrawal" => Ok(Self::CashWithdrawal),
            "loan_payment" => Ok(Self::LoanPayment),
            "reversal" => Ok(Self::Reversal),
            _ => Err(DomainError::InvariantViolation {
                message: "Unknown transaction kind".to_owned(),
            }),
        }
    }
}

impl TryFrom<String> for TransactionKind {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}
