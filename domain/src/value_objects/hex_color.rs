//! `HexColor` — validated `#RRGGBB` hex color normalized to uppercase (`VO-16`).

use nutype::nutype;

/// Validated `#RRGGBB` hex color code normalized to uppercase.
#[nutype(
    sanitize(trim, uppercase),
    validate(predicate = is_valid_hex_color),
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
pub struct HexColor(String);

impl HexColor {
    /// Returns the normalized `#RRGGBB` string slice.
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

fn is_valid_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}
