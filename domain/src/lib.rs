//! # FamilyLedger — Domain Crate
//!
//! This crate contains **all pure domain logic** for FamilyLedger.
//! It has zero I/O, database, or HTTP dependencies — everything here is
//! unit-testable in isolation.
//!
//! ## Architecture (DDD / Clean Architecture)
//!
//! ```text
//! domain/
//! ├── entities/       — Aggregate roots and child entities
//! │   └── family.rs       (Family, FamilyMember, InviteToken)
//! ├── value_objects/  — Immutable domain values with invariants
//! │   ├── currency_code.rs
//! │   ├── display_name.rs
//! │   ├── email.rs
//! │   ├── family_name.rs
//! │   ├── ids.rs          (Typed UUID v4 / v7 identifiers)
//! │   ├── invite_ttl.rs
//! │   ├── money.rs
//! │   ├── relationship.rs
//! │   └── role.rs         (Role, Permission, DomainAction)
//! ├── events/         — Domain events emitted by aggregates
//! │   └── mod.rs
//! ├── errors.rs       — Domain-level error types
//! └── lib.rs          — Public API of the crate
//! ```
//!
//! ## Non-Negotiable Rules
//!
//! - Money is **always** [`rust_decimal::Decimal`] — never `f64` or `f32`
//! - All PKs are strongly-typed UUID v4/v7 wrappers from [`value_objects::ids`]
//! - Soft-deletes only: `deleted_at: Option<DateTime<Utc>>`
//! - `family_id` is **never** trusted from unverified client payloads — derived from
//!   `X-Family-Id` and verified against active `family_members` membership (ADR-0011)

pub mod entities;
pub mod errors;
pub mod events;
pub mod ports;
pub mod value_objects;
