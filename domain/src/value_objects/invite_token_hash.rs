//! `InviteTokenHash` — SHA-256 hex digest of a bearer [`InviteTokenId`].
//!
//! Persisted in `family_invite_tokens.token_hash` (`CHAR(64)`) and emitted in domain
//! events so raw bearer invite tokens are never stored in DSQL or broadcast to EventBridge.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{errors::DomainError, value_objects::InviteTokenId};

const SHA256_HEX_LEN: usize = 64;

/// Validated 64-character lowercase ASCII hex SHA-256 digest of an [`InviteTokenId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct InviteTokenHash(String);

impl InviteTokenHash {
    /// Validates and constructs an `InviteTokenHash` from a hex digest string.
    pub fn try_new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let trimmed = raw.into().trim().to_owned();
        if !is_valid_sha256_hex(&trimmed) {
            return Err(DomainError::InvariantViolation {
                message: "Invite token hash must be 64 lowercase ASCII hex characters".to_string(),
            });
        }
        Ok(Self(trimmed))
    }

    /// Computes the deterministic SHA-256 lowercase hex digest of `token`.
    #[must_use]
    pub fn from_token_id(token: InviteTokenId) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(token.to_string().as_bytes());
        let digest = hasher.finalize();
        Self(encode_lowercase_hex(&digest))
    }

    /// Returns the inner lowercase hex string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` iff `token`'s SHA-256 digest equals this stored hash.
    #[must_use]
    pub fn matches_token(&self, token: InviteTokenId) -> bool {
        constant_time_eq(
            self.as_str().as_bytes(),
            Self::from_token_id(token).as_str().as_bytes(),
        )
    }
}

impl TryFrom<String> for InviteTokenHash {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl AsRef<str> for InviteTokenHash {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InviteTokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == SHA256_HEX_LEN
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn encode_lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_token_id_is_deterministic_64_lowercase_hex_and_matches() {
        let token = InviteTokenId::new_v4();
        let other = InviteTokenId::new_v4();

        let h1 = InviteTokenHash::from_token_id(token);
        let h2 = InviteTokenHash::from_token_id(token);
        assert_eq!(h1, h2);
        assert_eq!(h1.as_str().len(), 64);
        assert!(
            h1.as_str()
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        );
        assert!(h1.matches_token(token));
        assert!(!h1.matches_token(other));
    }

    #[test]
    fn try_new_and_serde_reject_uppercase_wrong_len_or_non_hex() {
        let upper = "A".repeat(64);
        let short = "a".repeat(63);
        let non_hex = format!("{}g", "a".repeat(63));

        assert!(InviteTokenHash::try_new(&upper).is_err());
        assert!(InviteTokenHash::try_new(&short).is_err());
        assert!(InviteTokenHash::try_new(&non_hex).is_err());
        assert!(serde_json::from_str::<InviteTokenHash>(&format!("\"{upper}\"")).is_err());

        let valid = "a".repeat(64);
        let parsed: InviteTokenHash = serde_json::from_str(&format!("\"{valid}\"")).unwrap();
        assert_eq!(parsed.as_str(), valid);
    }
}
