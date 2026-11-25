//! `Relationship` — validated family member relationship label value object.
//!
//! Backed by [`nutype`]: validation runs at construction and deserialization time
//! so it is impossible to hold a blank or oversized `Relationship` label.
//!
//! # Invariants
//! - Leading/trailing whitespace is trimmed automatically.
//! - The trimmed value must not be empty.
//! - Maximum 50 Unicode characters (matches `family_members.relationship VARCHAR(50)`
//!   and `family_invite_tokens.relationship VARCHAR(50)`).
//!
//! # Examples
//! ```
//! use domain::value_objects::Relationship;
//!
//! let rel = Relationship::try_new("  Grandma  ").unwrap();
//! assert_eq!(rel.into_inner(), "Grandma");
//!
//! assert!(Relationship::try_new("").is_err());
//! assert!(Relationship::try_new("   ").is_err());
//! ```

use nutype::nutype;

#[nutype(
    sanitize(trim),
    validate(
        not_empty,
        len_char_max = 50,
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
pub struct Relationship(String);

impl Relationship {
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
    fn valid_relationships_are_accepted() {
        assert!(Relationship::try_new("Grandma").is_ok());
        assert!(Relationship::try_new("Spouse").is_ok());
        assert!(Relationship::try_new("Cousin").is_ok());
        assert!(Relationship::try_new("Бабушка").is_ok());
    }

    #[test]
    fn whitespace_is_trimmed() {
        let rel = Relationship::try_new("  Daughter  ").unwrap();
        assert_eq!(rel.into_inner(), "Daughter");
    }

    #[test]
    fn empty_or_whitespace_is_rejected() {
        assert!(Relationship::try_new("").is_err());
        assert!(Relationship::try_new("   ").is_err());
    }

    #[test]
    fn too_long_is_rejected() {
        let long = "a".repeat(51);
        assert!(Relationship::try_new(long).is_err());
    }

    #[test]
    fn exactly_50_chars_is_accepted() {
        let at_limit = "a".repeat(50);
        assert!(Relationship::try_new(at_limit).is_ok());
    }

    #[test]
    fn serde_validates_on_deserialization() {
        let rel: Relationship = serde_json::from_str("\"  Spouse  \"").unwrap();
        assert_eq!(rel.as_ref(), "Spouse");
        assert_eq!(serde_json::to_string(&rel).unwrap(), "\"Spouse\"");

        assert!(serde_json::from_str::<Relationship>("\"\"").is_err());
        assert!(serde_json::from_str::<Relationship>("\"   \"").is_err());
        assert!(serde_json::from_str::<Relationship>(&format!("\"{}\"", "a".repeat(51))).is_err());
    }

    #[test]
    fn control_chars_and_bidi_overrides_are_rejected() {
        for bad in [
            "Grandma\0",
            "Grandma\nSpouse",
            "Grandma\x1b",
            "Grandma\u{202E}Spouse",
        ] {
            assert!(Relationship::try_new(bad).is_err());
        }
    }
}
