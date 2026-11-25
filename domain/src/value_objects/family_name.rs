//! `FamilyName` — validated family name value object.
//!
//! Backed by [`nutype`]: validation runs at construction time so it is
//! impossible to hold an invalid `FamilyName` anywhere in the domain.
//!
//! # Invariants
//! - Leading/trailing whitespace is trimmed automatically.
//! - The trimmed value must not be empty.
//! - Maximum 200 Unicode characters (matches `family_families.name VARCHAR(200)`).
//!
//! # Examples
//! ```
//! use domain::value_objects::FamilyName;
//!
//! let name = FamilyName::try_new("  Smith Family  ").unwrap();
//! assert_eq!(name.into_inner(), "Smith Family");
//!
//! assert!(FamilyName::try_new("").is_err());
//! assert!(FamilyName::try_new("   ").is_err());
//! ```

use nutype::nutype;

#[nutype(
    sanitize(trim),
    validate(
        not_empty,
        len_char_max = 200,
        predicate = crate::value_objects::is_safe_display_text
    ),
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
pub struct FamilyName(String);

impl FamilyName {
    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_name_is_accepted() {
        assert!(FamilyName::try_new("Smith Family").is_ok());
        assert!(FamilyName::try_new("Família da Silva").is_ok());
    }

    #[test]
    fn whitespace_is_trimmed() {
        let name = FamilyName::try_new("  Smith  ").unwrap();
        assert_eq!(name.into_inner(), "Smith");
    }

    #[test]
    fn empty_string_is_rejected() {
        assert!(FamilyName::try_new("").is_err());
    }

    #[test]
    fn whitespace_only_is_rejected() {
        assert!(FamilyName::try_new("   ").is_err());
    }

    #[test]
    fn too_long_is_rejected() {
        let long = "a".repeat(201);
        assert!(FamilyName::try_new(long).is_err());
    }

    #[test]
    fn exactly_200_chars_is_accepted() {
        let at_limit = "a".repeat(200);
        assert!(FamilyName::try_new(at_limit).is_ok());
    }

    #[test]
    fn control_chars_and_bidi_overrides_are_rejected() {
        for bad in [
            "Smith\0Family",
            "Smith\nFamily",
            "Smith\rFamily",
            "Smith\x1b[31m",
            "Smith\u{202E}Family",
            "Smith\u{2066}Family",
        ] {
            assert!(FamilyName::try_new(bad).is_err());
        }
    }
}
