//! `InviteTtl` — strictly positive invite token expiration duration (in seconds).
//!
//! Backed by [`nutype`]: guarantees at construction and deserialization time that
//! an invite token always has a strictly positive expiration window (`> 0` seconds).
//!
//! # Examples
//! ```
//! use domain::value_objects::InviteTtl;
//!
//! let ttl = InviteTtl::from_hours(48).unwrap();
//! assert_eq!(ttl.into_inner(), 48 * 3_600);
//!
//! assert!(InviteTtl::try_new(0).is_err());
//! assert!(InviteTtl::from_hours(-1).is_err());
//! ```

use nutype::nutype;

use crate::errors::DomainError;

const SECONDS_PER_HOUR: i64 = 3_600;
const SECONDS_PER_DAY: i64 = 86_400;

#[nutype(
    validate(greater = 0),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        Display,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct InviteTtl(i64);

impl InviteTtl {
    /// Creates an `InviteTtl` from a positive number of hours.
    pub fn from_hours(hours: i64) -> Result<Self, DomainError> {
        let secs =
            hours
                .checked_mul(SECONDS_PER_HOUR)
                .ok_or_else(|| DomainError::InvalidInviteTtl {
                    reason: "Invite TTL in hours overflows i64 seconds".to_string(),
                })?;
        Self::try_new(secs).map_err(DomainError::from)
    }

    /// Creates an `InviteTtl` from a positive number of days.
    pub fn from_days(days: i64) -> Result<Self, DomainError> {
        let secs =
            days.checked_mul(SECONDS_PER_DAY)
                .ok_or_else(|| DomainError::InvalidInviteTtl {
                    reason: "Invite TTL in days overflows i64 seconds".to_string(),
                })?;
        Self::try_new(secs).map_err(DomainError::from)
    }

    /// Converts a [`chrono::Duration`] into an `InviteTtl`, rejecting non-positive or sub-second durations.
    pub fn try_from_duration(duration: chrono::Duration) -> Result<Self, DomainError> {
        if duration <= chrono::Duration::zero() {
            return Err(DomainError::InvalidInviteTtl {
                reason: "Invite expiration duration must be positive".to_string(),
            });
        }
        let secs = duration.num_seconds();
        if secs <= 0 {
            return Err(DomainError::InvalidInviteTtl {
                reason: "Invite expiration duration must be at least 1 second".to_string(),
            });
        }
        Self::try_new(secs).map_err(DomainError::from)
    }

    /// Returns the expiration duration as a [`chrono::Duration`].
    pub fn as_duration(&self) -> chrono::Duration {
        chrono::Duration::seconds(self.into_inner())
    }
}

impl TryFrom<chrono::Duration> for InviteTtl {
    type Error = DomainError;

    fn try_from(duration: chrono::Duration) -> Result<Self, Self::Error> {
        Self::try_from_duration(duration)
    }
}

impl From<InviteTtl> for chrono::Duration {
    fn from(ttl: InviteTtl) -> Self {
        ttl.as_duration()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_durations_are_accepted() {
        let one_sec = InviteTtl::try_new(1).unwrap();
        assert_eq!(one_sec.into_inner(), 1);
        assert_eq!(one_sec.as_duration(), chrono::Duration::seconds(1));

        let two_days_hours = InviteTtl::from_hours(48).unwrap();
        let two_days = InviteTtl::from_days(2).unwrap();
        assert_eq!(two_days_hours, two_days);
        assert_eq!(chrono::Duration::from(two_days), chrono::Duration::days(2));
    }

    #[test]
    fn zero_and_negative_are_rejected() {
        assert!(InviteTtl::try_new(0).is_err());
        assert!(InviteTtl::try_new(-10).is_err());
        assert!(InviteTtl::from_hours(0).is_err());
        assert!(InviteTtl::from_hours(-1).is_err());
        assert!(InviteTtl::from_days(0).is_err());
        assert!(InviteTtl::from_days(-1).is_err());
        assert!(InviteTtl::try_from(chrono::Duration::zero()).is_err());
        assert!(InviteTtl::try_from(chrono::Duration::hours(-1)).is_err());
    }

    #[test]
    fn subsecond_positive_duration_is_rejected() {
        let half_sec = chrono::Duration::milliseconds(500);
        assert!(InviteTtl::try_from_duration(half_sec).is_err());
    }

    #[test]
    fn overflow_is_rejected() {
        assert!(InviteTtl::from_hours(i64::MAX).is_err());
        assert!(InviteTtl::from_days(i64::MAX).is_err());
    }

    #[test]
    fn serde_validates_on_deserialization() {
        let ttl: InviteTtl = serde_json::from_str("3600").unwrap();
        assert_eq!(ttl.into_inner(), 3_600);
        assert_eq!(serde_json::to_string(&ttl).unwrap(), "3600");

        assert!(serde_json::from_str::<InviteTtl>("0").is_err());
        assert!(serde_json::from_str::<InviteTtl>("-10").is_err());
    }
}
