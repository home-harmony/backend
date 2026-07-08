//! Value objects — immutable domain values that enforce their own invariants.
//!
//! Value objects have no identity (no UUID). Two Money values with the same
//! amount and currency are equal. They are the building blocks of entities.

pub mod currency_code;
pub mod display_name;
pub mod family_name;
pub mod money;
pub mod role;

pub use currency_code::CurrencyCode;
pub use display_name::DisplayName;
pub use family_name::FamilyName;
pub use money::Money;
pub use role::Role;
