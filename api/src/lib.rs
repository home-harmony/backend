//! # API Crate
//!
//! Shared Axum middleware, JWT validation, response helpers, and error mapping
//! for all FamilyLedger Lambda functions.
//!
//! ## Modules
//!
//! - [`auth`]: JWT extraction and validation; `AuthClaims` extractor for Axum handlers
//! - [`errors`]: Maps domain errors to HTTP responses
//! - [`response`]: Standard JSON response helpers

pub mod auth;
pub mod errors;
pub mod response;
