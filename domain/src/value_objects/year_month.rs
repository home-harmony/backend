//! `YearMonth` — validated `'YYYY-MM'` calendar month with exact UTC month-end cutoff (`VO-17`).

use std::fmt;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::errors::DomainError;

const MIN_YEAR: i32 = 1970;
const MAX_YEAR: i32 = 9999;

/// A validated calendar month in `'YYYY-MM'` format (`1970-01` ..= `9999-12`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct YearMonth {
    year: i32,
    month: u32,
}

impl YearMonth {
    /// Parses and validates a `'YYYY-MM'` string (trimming surrounding whitespace).
    pub fn try_new(raw: impl AsRef<str>) -> Result<Self, DomainError> {
        let trimmed = raw.as_ref().trim();
        let (year_str, month_str) = split_year_month(trimmed)?;
        let year: i32 = year_str.parse().map_err(|_| invalid_ym())?;
        let month: u32 = month_str.parse().map_err(|_| invalid_ym())?;
        Self::from_ym(year, month)
    }

    /// Constructs a `YearMonth` from numeric `(year, month)`.
    pub fn from_ym(year: i32, month: u32) -> Result<Self, DomainError> {
        if !(MIN_YEAR..=MAX_YEAR).contains(&year) || !(1..=12).contains(&month) {
            return Err(invalid_ym());
        }
        Ok(Self { year, month })
    }

    /// Returns the 4-digit year (`1970..=9999`).
    #[must_use]
    pub const fn year(self) -> i32 {
        self.year
    }

    /// Returns the month number (`1..=12`).
    #[must_use]
    pub const fn month(self) -> u32 {
        self.month
    }

    /// Returns the canonical `'YYYY-MM'` string representation.
    #[must_use]
    pub fn as_str(&self) -> String {
        self.to_string()
    }

    /// Returns the exact last microsecond of this calendar month in UTC (`YYYY-MM-DDT23:59:59.999999Z`).
    #[must_use]
    pub fn end_of_month_utc(&self) -> DateTime<Utc> {
        let (next_year, next_month) = if self.month == 12 {
            (self.year + 1, 1)
        } else {
            (self.year, self.month + 1)
        };
        NaiveDate::from_ymd_opt(next_year, next_month, 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|dt| dt.and_utc() - Duration::microseconds(1))
            .unwrap_or(DateTime::UNIX_EPOCH)
    }
}

impl From<YearMonth> for String {
    fn from(ym: YearMonth) -> Self {
        ym.to_string()
    }
}

impl TryFrom<String> for YearMonth {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl TryFrom<&str> for YearMonth {
    type Error = DomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl fmt::Display for YearMonth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

fn split_year_month(value: &str) -> Result<(&str, &str), DomainError> {
    let bytes = value.as_bytes();
    let valid_shape = bytes.len() == 7
        && bytes[4] == b'-'
        && bytes[..4].iter().all(|b| b.is_ascii_digit())
        && bytes[5..].iter().all(|b| b.is_ascii_digit());
    if !valid_shape {
        return Err(invalid_ym());
    }
    Ok((&value[..4], &value[5..]))
}

fn invalid_ym() -> DomainError {
    DomainError::InvalidYearMonth {
        reason: "YearMonth must be formatted as YYYY-MM (1970-01 to 9999-12)".to_owned(),
    }
}
