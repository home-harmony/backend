//! `DisplayName` — validated member display name value object.
//!
//! Backed by [`nutype`]: validation runs at construction time so it is
//! impossible to hold a blank `DisplayName` on any `FamilyMember`.
//!
//! # Invariants
//! - Leading/trailing whitespace is trimmed automatically.
//! - The trimmed value must not be empty.
//! - Maximum 100 Unicode characters (matches `family_members.display_name VARCHAR(100)`).
//!
//! # Examples
//! ```
//! use domain::value_objects::DisplayName;
//!
//! let name = DisplayName::try_new("  Alice  ").unwrap();
//! assert_eq!(name.into_inner(), "Alice");
//!
//! assert!(DisplayName::try_new("").is_err());
//! assert!(DisplayName::try_new("   ").is_err());
//! ```

use nutype::nutype;

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 100),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        AsRef,
        Display,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct DisplayName(String);

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_name_is_accepted() {
        assert!(DisplayName::try_new("Alice").is_ok());
        assert!(DisplayName::try_new("João Pedro").is_ok());
    }

    #[test]
    fn whitespace_is_trimmed() {
        let name = DisplayName::try_new("  Bob  ").unwrap();
        assert_eq!(name.into_inner(), "Bob");
    }

    #[test]
    fn empty_string_is_rejected() {
        assert!(DisplayName::try_new("").is_err());
    }

    #[test]
    fn whitespace_only_is_rejected() {
        assert!(DisplayName::try_new("   ").is_err());
    }

    #[test]
    fn too_long_is_rejected() {
        let long = "a".repeat(101);
        assert!(DisplayName::try_new(long).is_err());
    }

    #[test]
    fn exactly_100_chars_is_accepted() {
        let at_limit = "a".repeat(100);
        assert!(DisplayName::try_new(at_limit).is_ok());
    }
}
