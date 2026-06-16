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
//! ├── entities/       — Aggregate roots and entities
//! │   ├── family.rs
//! │   ├── member.rs
//! │   └── invite_token.rs
//! ├── value_objects/  — Immutable domain values with invariants
//! │   ├── money.rs
//! │   ├── currency_code.rs
//! │   └── role.rs
//! ├── events/         — Domain events emitted by aggregates
//! │   └── mod.rs
//! ├── errors.rs       — Domain-level error types
//! └── lib.rs          — Public API of the crate
//! ```
//!
//! ## Non-Negotiable Rules
//!
//! - Money is **always** [`rust_decimal::Decimal`] — never `f64` or `f32`
//! - All PKs are [`uuid::Uuid`] (v4 or v7)
//! - Soft-deletes only: `deleted_at: Option<DateTime<Utc>>`
//! - `family_id` is **never** trusted from client input — extracted from JWT in the API layer

pub mod entities;
pub mod errors;
pub mod events;
pub mod value_objects;
