//! # Infrastructure Crate
//!
//! Database repositories (SQLx adapters), external service clients,
//! and I/O wrappers for FamilyLedger.
//!
//! ## Structure
//!
//! ```text
//! infrastructure/
//! ├── db/
//! │   ├── pool.rs         — Aurora DSQL connection pool setup
//! │   └── pagination.rs   — Keyset cursor encoding/decoding utilities
//! └── lib.rs
//! ```
//!
//! ## Database Rules (Aurora DSQL)
//!
//! - All reads use `BEGIN READ ONLY` to avoid OCC conflict overhead
//! - All writes use `aurora_dsql_sqlx_connector::retry_on_occ` for automatic retry
//! - Page size ≤ 50 for UI queries; batch writes ≤ 500 rows per transaction
//! - OFFSET pagination is banned — use keyset cursors only

pub mod db;
