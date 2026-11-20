//! Domain errors — strongly-typed error variants for all domain invariant violations.

use rust_decimal::Decimal;
use thiserror::Error;

use crate::value_objects::{
    CurrencyCode, MemberId, Role, UserId, display_name::DisplayNameError, email::EmailError,
    family_name::FamilyNameError, invite_ttl::InviteTtlError, relationship::RelationshipError,
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

    #[error("Invalid currency code '{code}': must be exactly 3 uppercase ASCII letters")]
    InvalidCurrencyCode { code: String },

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

    #[error("Family already has an owner and cannot have a second one")]
    DuplicateOwner,

    #[error("Member not found in family: {member_id}")]
    MemberNotFound { member_id: MemberId },

    #[error("User is not an active member of this family: {user_id}")]
    UserNotInFamily { user_id: UserId },

    #[error("Cannot remove the family owner")]
    CannotRemoveOwner,

    // ── Invite tokens ────────────────────────────────────────────────────────────
    #[error("Invite token has expired")]
    TokenExpired,

    #[error("Invite token has already been used")]
    TokenAlreadyUsed,

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
}

impl From<FamilyNameError> for DomainError {
    fn from(err: FamilyNameError) -> Self {
        Self::InvalidFamilyName {
            reason: err.to_string(),
        }
    }
}

impl From<DisplayNameError> for DomainError {
    fn from(err: DisplayNameError) -> Self {
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
