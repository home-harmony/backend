//! Value objects — immutable domain values that enforce their own invariants.
//!
//! Value objects are the building blocks of entities. Strongly-typed identifiers
//! (`FamilyId`, `UserId`, `TransactionId`, etc.) prevent type-confusion bugs, while
//! types like [`Money`], [`Role`], and [`MemberDisplayName`] encapsulate business validation.

pub mod currency_code;
pub mod email;
pub mod family_name;
pub mod ids;
pub mod invite_ttl;
pub mod member_display_name;
pub mod money;
pub mod relationship;
pub mod role;

pub use currency_code::CurrencyCode;
pub use email::Email;
pub use family_name::FamilyName;
pub use ids::*;
pub use invite_ttl::{InviteTtl, MAX_INVITE_TTL_SECONDS};
pub use member_display_name::MemberDisplayName;
pub use money::Money;
pub use relationship::Relationship;
pub use role::{DomainAction, Permission, Role};
