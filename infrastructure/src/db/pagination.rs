//! Keyset (cursor-based) pagination utilities.
//!
//! ## Why not OFFSET?
//!
//! Aurora DSQL's distributed architecture makes `OFFSET N` scan and discard N rows,
//! consuming DPUs proportional to offset depth. At page 100 with 50 items/page,
//! you've scanned 5,000 rows just to throw them away. OFFSET is **banned**.
//!
//! ## Cursor Pattern
//!
//! Instead, we encode the last row's sort key as an opaque base64 token.
//! The next query uses `WHERE (occurred_at, id) < ($cursor_time, $cursor_id)`.
//! This is O(1) regardless of how deep into the list you are.
//!
//! ## API Response Shape
//!
//! ```json
//! {
//!   "items": [...],
//!   "next_cursor": "eyJ0IjoiMjAyNi0wNi0wMVQxMjowMDowMFoiLCJpIjoiMTIzLi4uIn0",
//!   "has_more": true
//! }
//! ```
//!
//! The cursor is opaque to clients — they pass it back as-is.

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// Maximum number of items returned per page for UI queries.
/// RULE (AGENTS.md §2 & ADR-0002): Page size ≤ 50.
pub const MAX_PAGE_SIZE: u32 = 50;

/// Default page size if the client doesn't specify one.
pub const DEFAULT_PAGE_SIZE: u32 = 20;

/// Validated page size bounded to `1..=MAX_PAGE_SIZE` (`50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32")]
pub struct PageLimit(u32);

/// A pagination cursor holding the sort key of the last seen row.
///
/// Serializes to and deserializes from an opaque URL-safe base64 token on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageCursor {
    /// Timestamp of the last item (`occurred_at` for transactions).
    pub timestamp: DateTime<Utc>,
    /// UUID of the last item — used as a stable tiebreaker.
    pub id: Uuid,
}

/// Standard paginated API response wrapper.
#[derive(Debug, Serialize)]
pub struct Page<T: Serialize> {
    pub items: Vec<T>,
    /// Cursor for the next page (serialized as an opaque base64 token).
    /// `None` if this is the last page.
    pub next_cursor: Option<PageCursor>,
    pub has_more: bool,
}

/// Internal JSON payload encoded inside the base64 cursor token.
#[derive(Deserialize)]
struct CursorPayload {
    #[serde(rename = "t")]
    timestamp: DateTime<Utc>,
    #[serde(rename = "i")]
    id: Uuid,
}

impl PageLimit {
    /// Validates that `limit` is within `1..=MAX_PAGE_SIZE`.
    ///
    /// # Errors
    ///
    /// Returns an error if `limit == 0` or `limit > MAX_PAGE_SIZE`.
    pub fn try_new(limit: u32) -> Result<Self> {
        if limit == 0 || limit > MAX_PAGE_SIZE {
            anyhow::bail!("Page limit must be between 1 and {MAX_PAGE_SIZE}");
        }
        Ok(Self(limit))
    }

    /// Returns the limit as `usize` for slice/page truncation.
    #[must_use]
    pub const fn into_usize(self) -> usize {
        self.0 as usize
    }

    /// Returns `limit + 1` as `i64` for SQL `LIMIT` fetch-N+1 queries.
    #[must_use]
    pub const fn query_limit_plus_one(self) -> i64 {
        (self.0 as i64) + 1
    }
}

impl Default for PageLimit {
    fn default() -> Self {
        Self(DEFAULT_PAGE_SIZE)
    }
}

impl TryFrom<u32> for PageLimit {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self> {
        Self::try_new(value)
    }
}

impl PageCursor {
    /// Creates a new pagination cursor from the last row's sort key.
    #[must_use]
    pub const fn new(timestamp: DateTime<Utc>, id: Uuid) -> Self {
        Self { timestamp, id }
    }

    /// Encodes the cursor as a URL-safe base64 string.
    ///
    /// Returns an opaque token the client passes back on the next request.
    #[must_use]
    pub fn encode(&self) -> String {
        let payload = serde_json::json!({
            "t": self.timestamp.to_rfc3339(),
            "i": self.id.to_string()
        });
        URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes())
    }

    /// Decodes a cursor token from the client.
    ///
    /// # Errors
    /// Returns an error if the token is malformed or tampered with.
    /// Handlers should return HTTP 400 on cursor decode failure.
    pub fn decode(token: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(token)
            .context("Invalid cursor: base64 decode failed")?;
        let payload: CursorPayload =
            serde_json::from_slice(&bytes).context("Invalid cursor: JSON decode failed")?;
        Ok(Self::new(payload.timestamp, payload.id))
    }
}

impl<T: Serialize> Page<T> {
    /// Builds a [`Page`] using a validated [`PageLimit`] (`1..=50`).
    pub fn from_limited_query_result(
        items_plus_one: Vec<T>,
        limit: PageLimit,
        cursor_fn: impl Fn(&T) -> (DateTime<Utc>, Uuid),
    ) -> Self {
        Self::from_query_result(items_plus_one, limit.into_usize(), cursor_fn)
    }

    /// Creates a `Page` using the "fetch N+1" trick.
    ///
    /// Pass `page_size + 1` items from the DB query. If you get `page_size + 1`
    /// results back, there is a next page — truncate to `page_size` and build the cursor.
    ///
    /// # Arguments
    ///
    /// - `items_plus_one`: Results from `LIMIT page_size + 1` query
    /// - `page_size`: Desired page size (≤ 50)
    /// - `cursor_fn`: Closure that extracts `(DateTime<Utc>, Uuid)` from the last item
    pub fn from_query_result(
        mut items_plus_one: Vec<T>,
        page_size: usize,
        cursor_fn: impl Fn(&T) -> (DateTime<Utc>, Uuid),
    ) -> Self {
        let has_more = items_plus_one.len() > page_size;

        if has_more {
            items_plus_one.truncate(page_size);
        }

        let next_cursor = if has_more {
            items_plus_one.last().map(|last| {
                let (ts, id) = cursor_fn(last);
                PageCursor::new(ts, id)
            })
        } else {
            None
        };

        Page {
            items: items_plus_one,
            next_cursor,
            has_more,
        }
    }
}

impl Serialize for PageCursor {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.encode())
    }
}

impl<'de> Deserialize<'de> for PageCursor {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let token = String::deserialize(deserializer)?;
        Self::decode(&token).map_err(serde::de::Error::custom)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn cursor_round_trip() {
        let ts = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        let id = Uuid::new_v4();
        let cursor = PageCursor::new(ts, id);

        let token = cursor.encode();
        let decoded = PageCursor::decode(&token).unwrap();

        assert_eq!(decoded, cursor);
    }

    #[test]
    fn cursor_serde_serializes_as_opaque_base64_string() {
        let ts = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        let id = Uuid::new_v4();
        let cursor = PageCursor::new(ts, id);

        let json = serde_json::to_value(cursor).unwrap();
        assert_eq!(json, serde_json::Value::String(cursor.encode()));

        let round_tripped: PageCursor = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, cursor);
    }

    #[test]
    fn invalid_cursor_returns_error() {
        assert!(PageCursor::decode("not-valid-base64!!").is_err());
        assert!(PageCursor::decode("aW52YWxpZEpTT04=").is_err()); // valid b64 but invalid JSON
    }

    #[test]
    fn page_has_more_when_extra_item_returned() {
        let ts = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        let id = Uuid::new_v4();
        let items: Vec<u32> = (1..=21).collect(); // 21 items, page size 20
        let page = Page::from_query_result(items, 20, |_| (ts, id));
        assert!(page.has_more);
        assert_eq!(page.items.len(), 20);
        assert_eq!(page.next_cursor, Some(PageCursor::new(ts, id)));
    }

    #[test]
    fn page_has_no_more_when_exact_count_returned() {
        let items: Vec<u32> = (1..=20).collect(); // exactly 20, no extra
        let page = Page::from_query_result(items, 20, |_| (Utc::now(), Uuid::new_v4()));
        assert!(!page.has_more);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn page_limit_enforces_bounds_1_to_50() {
        assert!(PageLimit::try_new(0).is_err());
        assert!(PageLimit::try_new(51).is_err());
        assert_eq!(PageLimit::default().into_usize(), 20);
        assert_eq!(PageLimit::try_new(50).unwrap().query_limit_plus_one(), 51);

        let items: Vec<u32> = (1..=3).collect();
        let page = Page::from_limited_query_result(
            items,
            PageLimit::try_new(2).unwrap(),
            |_| (Utc::now(), Uuid::new_v4()),
        );
        assert!(page.has_more);
        assert_eq!(page.items.len(), 2);
    }
}
