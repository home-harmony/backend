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
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Maximum number of items returned per page for UI queries.
/// RULE (GEMINI.md §4): Page size ≤ 50.
pub const MAX_PAGE_SIZE: u32 = 50;

/// Default page size if the client doesn't specify one.
pub const DEFAULT_PAGE_SIZE: u32 = 20;

/// A decoded pagination cursor holding the sort key of the last seen row.
#[derive(Debug, Serialize, Deserialize)]
pub struct PageCursor {
    /// Timestamp of the last item (`occurred_at` for transactions).
    #[serde(rename = "t")]
    pub timestamp: DateTime<Utc>,
    /// UUID of the last item — used as a stable tiebreaker.
    #[serde(rename = "i")]
    pub id: Uuid,
}

impl PageCursor {
    /// Encodes the cursor as a URL-safe base64 string.
    ///
    /// Returns an opaque token the client passes back on the next request.
    pub fn encode(timestamp: DateTime<Utc>, id: Uuid) -> String {
        let payload = serde_json::json!({
            "t": timestamp.to_rfc3339(),
            "i": id.to_string()
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
        let cursor: Self =
            serde_json::from_slice(&bytes).context("Invalid cursor: JSON decode failed")?;
        Ok(cursor)
    }
}

/// Standard paginated API response wrapper.
#[derive(Debug, Serialize)]
pub struct Page<T: Serialize> {
    pub items: Vec<T>,
    /// Opaque base64 token for the next page. `None` if this is the last page.
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl<T: Serialize> Page<T> {
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
                PageCursor::encode(ts, id)
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

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn cursor_round_trip() {
        let ts = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        let id = Uuid::new_v4();

        let token = PageCursor::encode(ts, id);
        let decoded = PageCursor::decode(&token).unwrap();

        // Timestamps are serialized/deserialized through RFC 3339 string — sub-second precision
        assert_eq!(decoded.timestamp.timestamp(), ts.timestamp());
        assert_eq!(decoded.id, id);
    }

    #[test]
    fn invalid_cursor_returns_error() {
        assert!(PageCursor::decode("not-valid-base64!!").is_err());
        assert!(PageCursor::decode("aW52YWxpZEpTT04=").is_err()); // valid b64 but invalid JSON
    }

    #[test]
    fn page_has_more_when_extra_item_returned() {
        let items: Vec<u32> = (1..=21).collect(); // 21 items, page size 20
        let page = Page::from_query_result(items, 20, |_| (Utc::now(), Uuid::new_v4()));
        assert!(page.has_more);
        assert_eq!(page.items.len(), 20);
        assert!(page.next_cursor.is_some());
    }

    #[test]
    fn page_has_no_more_when_exact_count_returned() {
        let items: Vec<u32> = (1..=20).collect(); // exactly 20, no extra
        let page = Page::from_query_result(items, 20, |_| (Utc::now(), Uuid::new_v4()));
        assert!(!page.has_more);
        assert!(page.next_cursor.is_none());
    }
}
