//! Domain entities — aggregate roots and their child entities.
//!
//! Each aggregate root enforces its own invariants via methods.
//! No entity depends on infrastructure (database, HTTP).

pub mod family;

pub use family::{Family, FamilyMember, InviteToken};
