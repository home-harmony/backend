//! Domain errors — strongly-typed error variants for all domain invariant violations.

use thiserror::Error;

/// Errors that can occur within the domain layer.
/// These are pure business-rule violations, not I/O or infrastructure errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    // ── Money / Currency ──────────────────────────────────────────────────────
    #[error("Cannot mix currencies: expected {expected}, got {actual}")]
    CurrencyMismatch { expected: String, actual: String },

    #[error("Amount cannot be negative: {amount}")]
    NegativeAmount { amount: String },

    #[error("Invalid currency code '{code}': must be exactly 3 uppercase ASCII letters")]
    InvalidCurrencyCode { code: String },

    // ── Family ────────────────────────────────────────────────────────────────
    #[error("Invalid family name: {reason}")]
    InvalidFamilyName { reason: String },

    #[error("Invalid display name: {reason}")]
    InvalidDisplayName { reason: String },

    #[error("Family already has an owner and cannot have a second one")]
    DuplicateOwner,

    #[error("Member not found in family: {member_id}")]
    MemberNotFound { member_id: String },

    #[error("Cannot remove the family owner")]
    CannotRemoveOwner,

    // ── Invite tokens ─────────────────────────────────────────────────────────
    #[error("Invite token has expired")]
    TokenExpired,

    #[error("Invite token has already been used")]
    TokenAlreadyUsed,

    // ── Authorization ─────────────────────────────────────────────────────────
    #[error("Role '{role}' is not permitted to perform this action")]
    InsufficientRole { role: String },

    // ── Generic ───────────────────────────────────────────────────────────────
    #[error("Invariant violation: {message}")]
    InvariantViolation { message: String },
}
