//! Value objects — immutable domain values that enforce their own invariants.
//!
//! Value objects are the building blocks of entities. Strongly-typed identifiers
//! (`FamilyId`, `UserId`, `TransactionId`, etc.) prevent type-confusion bugs, while
//! types like [`Money`], [`Role`], and [`DisplayName`] encapsulate business validation.

pub mod currency_code;
pub mod display_name;
pub mod family_name;
pub mod ids;
pub mod money;
pub mod role;

pub use currency_code::CurrencyCode;
pub use display_name::DisplayName;
pub use family_name::FamilyName;
pub use ids::*;
pub use money::Money;
pub use role::Role;
