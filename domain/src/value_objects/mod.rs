//! Value objects — immutable domain values that enforce their own invariants.
//!
//! Value objects are the building blocks of entities. Strongly-typed identifiers
//! (`FamilyId`, `UserId`, `TransactionId`, etc.) prevent type-confusion bugs, while
//! types like [`Money`], [`Role`], and [`DisplayName`] encapsulate business validation.

pub mod currency_code;
pub mod display_name;
pub mod email;
pub mod family_name;
pub mod ids;
pub mod invite_token_hash;
pub mod invite_ttl;
pub mod money;
pub mod relationship;
pub mod role;

pub use currency_code::CurrencyCode;
pub use display_name::DisplayName;
pub use email::Email;
pub use family_name::FamilyName;
pub use ids::*;
pub use invite_token_hash::InviteTokenHash;
pub use invite_ttl::{InviteTtl, MAX_INVITE_TTL_SECONDS};
pub use money::Money;
pub use relationship::Relationship;
pub use role::{DomainAction, Permission, Role};

/// Rejects ASCII/Unicode control characters and Unicode BiDi formatting overrides
/// to prevent UI text spoofing and indirect prompt/delimiter injection.
pub(crate) fn is_safe_display_text(value: &str) -> bool {
    !value
        .chars()
        .any(|c| c.is_control() || is_bidi_override(c))
}

const fn is_bidi_override(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
    )
}
