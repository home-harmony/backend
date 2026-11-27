//! Validated value objects for `Category` and `Transaction` text attributes (`VO-18`).

use nutype::nutype;

use crate::errors::DomainError;

/// Maximum number of deduplicated tags permitted on a single transaction.
pub const MAX_TAGS_PER_TRANSACTION: usize = 20;

/// Validated ledger category name (`1..=100` trimmed Unicode chars, control/BiDi safe).
#[nutype(
    sanitize(trim),
    validate(
        not_empty,
        len_char_max = 100,
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
pub struct CategoryName(String);

impl CategoryName {
    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

/// Validated ledger category icon identifier (`1..=50` trimmed Unicode chars, control/BiDi safe).
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
pub struct CategoryIcon(String);

impl CategoryIcon {
    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

/// Validated transaction tag (`1..=40` trimmed Unicode chars, control/BiDi safe).
#[nutype(
    sanitize(trim),
    validate(
        not_empty,
        len_char_max = 40,
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
pub struct Tag(String);

impl Tag {
    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

/// Validated transaction note/description (`1..=500` trimmed Unicode chars, control/BiDi safe).
#[nutype(
    sanitize(trim),
    validate(
        not_empty,
        len_char_max = 500,
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
pub struct TransactionDescription(String);

impl TransactionDescription {
    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

/// Validates and deduplicates transaction tags while enforcing [`MAX_TAGS_PER_TRANSACTION`].
pub fn validate_tags<I, S>(raw: I) -> Result<Vec<Tag>, DomainError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut deduped = Vec::new();
    for item in raw {
        let tag = Tag::try_new(item.as_ref())?;
        if !deduped.contains(&tag) {
            deduped.push(tag);
        }
        if deduped.len() > MAX_TAGS_PER_TRANSACTION {
            return Err(DomainError::TooManyTags {
                max: MAX_TAGS_PER_TRANSACTION,
            });
        }
    }
    Ok(deduped)
}
