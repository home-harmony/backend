//! Domain errors — strongly-typed error variants for all domain invariant violations.

use rust_decimal::Decimal;
use thiserror::Error;
use uuid::Uuid;

use crate::value_objects::{
    AccountId, AccountKind, CategoryId, CurrencyCode, FamilyId, MemberId, Role, TransactionId,
    UserId,
    account_fields::{AccountNameError, BankNameError, LastFourError},
    currency_code::CurrencyCodeError,
    email::EmailError,
    family_name::FamilyNameError,
    hex_color::HexColorError,
    invite_ttl::InviteTtlError,
    ledger_fields::{CategoryIconError, CategoryNameError, TagError, TransactionDescriptionError},
    member_display_name::MemberDisplayNameError,
    relationship::RelationshipError,
};

/// Errors that can occur within the domain layer.
/// These are pure business-rule violations, not I/O or infrastructure errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    // ── Money / Currency ──────────────────────────────────────────────────────────
    #[error("Cannot mix currencies: expected {expected}, got {actual}")]
    CurrencyMismatch {
        expected: CurrencyCode,
        actual: CurrencyCode,
    },

    #[error("Amount cannot be negative: {amount}")]
    NegativeAmount { amount: Decimal },

    #[error("Amount must be strictly positive: {amount}")]
    NonPositiveAmount { amount: Decimal },

    #[error("Arithmetic overflow during {operation}")]
    ArithmeticOverflow { operation: &'static str },

    #[error("Invalid currency code: {reason}")]
    InvalidCurrencyCode { reason: String },

    #[error("Invalid year-month: {reason}")]
    InvalidYearMonth { reason: String },

    #[error("Invalid hex color: {reason}")]
    InvalidHexColor { reason: String },

    // ── Family ───────────────────────────────────────────────────────────────────
    #[error("Invalid family name: {reason}")]
    InvalidFamilyName { reason: String },

    #[error("Invalid display name: {reason}")]
    InvalidDisplayName { reason: String },

    #[error("Invalid email: {reason}")]
    InvalidEmail { reason: String },

    #[error("Invalid relationship: {reason}")]
    InvalidRelationship { reason: String },

    #[error("Invalid invite TTL: {reason}")]
    InvalidInviteTtl { reason: String },

    #[error("Family not found: {family_id}")]
    FamilyNotFound { family_id: FamilyId },

    #[error("Family has been deleted")]
    FamilyDeleted,

    #[error("Family already has an owner and cannot have a second one")]
    DuplicateOwner,

    #[error("User is already an active member of this family: {user_id}")]
    MemberAlreadyExists { user_id: UserId },

    #[error("Member not found in family: {member_id}")]
    MemberNotFound { member_id: MemberId },

    #[error("User is not an active member of this family: {user_id}")]
    UserNotInFamily { user_id: UserId },

    #[error("Cannot remove the family owner")]
    CannotRemoveOwner,

    // ── Invite tokens ────────────────────────────────────────────────────────────
    #[error("Invite token not found")]
    InviteTokenNotFound,

    #[error("Invite token has expired")]
    TokenExpired,

    #[error("Invite token has already been used")]
    TokenAlreadyUsed,

    // ── Payment Accounts ─────────────────────────────────────────────────────────
    #[error("Invalid account name: {reason}")]
    InvalidAccountName { reason: String },

    #[error("Invalid bank name: {reason}")]
    InvalidBankName { reason: String },

    #[error("Invalid last four digits: {reason}")]
    InvalidLastFour { reason: String },

    #[error("Payment account not found: {account_id}")]
    AccountNotFound { account_id: AccountId },

    #[error("Payment account has been deleted: {account_id}")]
    AccountDeleted { account_id: AccountId },

    #[error("Credit limit is only allowed for credit card accounts, not '{kind}'")]
    CreditLimitNotAllowed { kind: AccountKind },

    // ── Ledger & Categories ──────────────────────────────────────────────────────
    #[error("Invalid category name: {reason}")]
    InvalidCategoryName { reason: String },

    #[error("Invalid category icon: {reason}")]
    InvalidCategoryIcon { reason: String },

    #[error("Invalid tag: {reason}")]
    InvalidTag { reason: String },

    #[error("Too many tags (maximum {max})")]
    TooManyTags { max: usize },

    #[error("Invalid transaction description: {reason}")]
    InvalidDescription { reason: String },

    #[error("Category not found: {category_id}")]
    CategoryNotFound { category_id: CategoryId },

    #[error("Category has been deleted: {category_id}")]
    CategoryDeleted { category_id: CategoryId },

    #[error("Cannot modify or delete system category: {category_id}")]
    CannotModifySystemCategory { category_id: CategoryId },

    #[error("Category kind mismatch: expected {expected}, got {actual}")]
    CategoryKindMismatch { expected: String, actual: String },

    #[error("Transaction not found: {transaction_id}")]
    TransactionNotFound { transaction_id: TransactionId },

    #[error("Transaction has been deleted: {transaction_id}")]
    TransactionDeleted { transaction_id: TransactionId },

    #[error("Transaction has already been amended: {transaction_id}")]
    TransactionAlreadyAmended { transaction_id: TransactionId },

    #[error("Cannot amend or delete a reversal transaction")]
    CannotAmendReversal,

    #[error("Source and destination accounts must be distinct: {account_id}")]
    SameAccountTransfer { account_id: AccountId },

    #[error("Cash withdrawal must move from a non-cash account to a cash account")]
    InvalidCashWithdrawalAccounts,

    #[error("Cross-currency transfer requires destination_amount")]
    CrossCurrencyDestinationRequired,

    #[error("Same-currency transfer must not specify destination_amount")]
    SameCurrencyDestinationForbidden,

    #[error("Idempotency key already used with a different payload: {idempotency_key}")]
    IdempotencyConflict { idempotency_key: Uuid },

    // ── Debt & Loans (ADR-0008) ──────────────────────────────────────────────────
    #[error(
        "Invalid payment split: principal ({principal}) + interest ({interest}) != total payment ({total})"
    )]
    InvalidPaymentSplit {
        principal: Decimal,
        interest: Decimal,
        total: Decimal,
    },

    // ── Authorization ────────────────────────────────────────────────────────────
    #[error("Role '{role}' is not permitted to perform this action")]
    InsufficientRole { role: Role },

    // ── Generic ──────────────────────────────────────────────────────────────────
    #[error("Invariant violation: {message}")]
    InvariantViolation { message: String },

    // ── Infrastructure / Port I/O ────────────────────────────────────────────────
    #[error("Internal error: {message}")]
    Internal { message: String },
}

impl From<CurrencyCodeError> for DomainError {
    fn from(err: CurrencyCodeError) -> Self {
        Self::InvalidCurrencyCode {
            reason: err.to_string(),
        }
    }
}

impl From<FamilyNameError> for DomainError {
    fn from(err: FamilyNameError) -> Self {
        Self::InvalidFamilyName {
            reason: err.to_string(),
        }
    }
}

impl From<MemberDisplayNameError> for DomainError {
    fn from(err: MemberDisplayNameError) -> Self {
        Self::InvalidDisplayName {
            reason: err.to_string(),
        }
    }
}

impl From<EmailError> for DomainError {
    fn from(err: EmailError) -> Self {
        Self::InvalidEmail {
            reason: err.to_string(),
        }
    }
}

impl From<RelationshipError> for DomainError {
    fn from(err: RelationshipError) -> Self {
        Self::InvalidRelationship {
            reason: err.to_string(),
        }
    }
}

impl From<InviteTtlError> for DomainError {
    fn from(err: InviteTtlError) -> Self {
        Self::InvalidInviteTtl {
            reason: err.to_string(),
        }
    }
}

impl From<AccountNameError> for DomainError {
    fn from(err: AccountNameError) -> Self {
        Self::InvalidAccountName {
            reason: err.to_string(),
        }
    }
}

impl From<BankNameError> for DomainError {
    fn from(err: BankNameError) -> Self {
        Self::InvalidBankName {
            reason: err.to_string(),
        }
    }
}

impl From<LastFourError> for DomainError {
    fn from(err: LastFourError) -> Self {
        Self::InvalidLastFour {
            reason: err.to_string(),
        }
    }
}

impl From<HexColorError> for DomainError {
    fn from(err: HexColorError) -> Self {
        Self::InvalidHexColor {
            reason: err.to_string(),
        }
    }
}

impl From<CategoryNameError> for DomainError {
    fn from(err: CategoryNameError) -> Self {
        Self::InvalidCategoryName {
            reason: err.to_string(),
        }
    }
}

impl From<CategoryIconError> for DomainError {
    fn from(err: CategoryIconError) -> Self {
        Self::InvalidCategoryIcon {
            reason: err.to_string(),
        }
    }
}

impl From<TagError> for DomainError {
    fn from(err: TagError) -> Self {
        Self::InvalidTag {
            reason: err.to_string(),
        }
    }
}

impl From<TransactionDescriptionError> for DomainError {
    fn from(err: TransactionDescriptionError) -> Self {
        Self::InvalidDescription {
            reason: err.to_string(),
        }
    }
}
