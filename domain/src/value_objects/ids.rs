//! Strongly-typed domain entity identifiers.
//!
//! # Primary Key Strategy (RFC 9562 & Aurora DSQL)
//!
//! In distributed relational databases like Aurora DSQL, selecting the appropriate
//! UUID version is critical for B-tree locality, write throughput, and security:
//!
//! - **UUID v4 (Random / Unpredictable)**: Used for security tokens (`InviteTokenId`),
//!   user identity (`UserId`), and low-frequency aggregate roots (`FamilyId`, `MemberId`,
//!   `AccountId`, `CategoryId`, `LoanId`, `RecurringPaymentId`, `BudgetId`, `GoalId`).
//!   Prevents timing leakage and tenant enumeration.
//!
//! - **UUID v7 (Time-Ordered / Monotonic)**: Used for high-throughput append-only logs,
//!   time-series records, and keyset-paginated entities (`TransactionId`, `LoanPaymentId`,
//!   `PlanId`, `RecurringPaymentRecordId`, `GoalContributionId`, `AccountBalanceSnapshotId`).
//!   Guarantees sequential B-tree writes and eliminates index fragmentation.

use std::fmt;
use std::str::FromStr;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id_v4 {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new random UUID v4 identifier.
            #[inline]
            pub fn new_v4() -> Self {
                Self(Uuid::new_v4())
            }

            /// Creates an identifier from an existing [`uuid::Uuid`].
            #[inline]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the inner [`uuid::Uuid`].
            #[inline]
            pub const fn into_inner(self) -> Uuid {
                self.0
            }

            /// Returns a reference to the inner [`uuid::Uuid`].
            #[inline]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            #[inline]
            fn default() -> Self {
                Self::new_v4()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }

        impl From<Uuid> for $name {
            #[inline]
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for Uuid {
            #[inline]
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl AsRef<Uuid> for $name {
            #[inline]
            fn as_ref(&self) -> &Uuid {
                &self.0
            }
        }
    };
}

macro_rules! define_id_v7 {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new time-ordered UUID v7 identifier (RFC 9562).
            #[inline]
            pub fn now_v7() -> Self {
                Self(Uuid::now_v7())
            }

            /// Creates an identifier from an existing [`uuid::Uuid`].
            #[inline]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the inner [`uuid::Uuid`].
            #[inline]
            pub const fn into_inner(self) -> Uuid {
                self.0
            }

            /// Returns a reference to the inner [`uuid::Uuid`].
            #[inline]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            #[inline]
            fn default() -> Self {
                Self::now_v7()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }

        impl From<Uuid> for $name {
            #[inline]
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for Uuid {
            #[inline]
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl AsRef<Uuid> for $name {
            #[inline]
            fn as_ref(&self) -> &Uuid {
                &self.0
            }
        }
    };
}

// ─── UUID v4 Identifiers (Random / Entropy-Driven) ───────────────────────────

define_id_v4!(
    FamilyId,
    "Unique identifier for a Family aggregate root (UUID v4)."
);

define_id_v4!(
    MemberId,
    "Unique identifier for a FamilyMember entity (UUID v4)."
);

define_id_v4!(
    UserId,
    "Unique identifier for a Cognito user account (UUID v4 from JWT `sub`)."
);

define_id_v4!(
    InviteTokenId,
    "Security token for prospective family members (UUID v4, zero timing leakage)."
);

define_id_v4!(
    AccountId,
    "Unique identifier for a payment instrument or bank account (UUID v4)."
);

define_id_v4!(
    CategoryId,
    "Unique identifier for a transaction category (UUID v4)."
);

define_id_v4!(
    LoanId,
    "Unique identifier for a registered loan or credit line (UUID v4)."
);

define_id_v4!(
    RecurringPaymentId,
    "Unique identifier for a recurring payment configuration (UUID v4)."
);

define_id_v4!(
    BudgetId,
    "Unique identifier for a monthly budget aggregate root (UUID v4)."
);

define_id_v4!(
    BudgetEnvelopeId,
    "Unique identifier for a budget envelope (UUID v4)."
);

define_id_v4!(
    GoalId,
    "Unique identifier for a savings goal (UUID v4)."
);

// ─── UUID v7 Identifiers (Time-Ordered / Monotonic) ──────────────────────────

define_id_v7!(
    TransactionId,
    "Unique time-sortable identifier for a financial transaction (UUID v7)."
);

define_id_v7!(
    LoanPaymentId,
    "Unique time-sortable identifier for an append-only loan payment record (UUID v7)."
);

define_id_v7!(
    PlanId,
    "Unique time-sortable identifier for a versioned debt repayment plan (UUID v7)."
);

define_id_v7!(
    RecurringPaymentRecordId,
    "Unique time-sortable identifier for a recurring payment execution log (UUID v7)."
);

define_id_v7!(
    GoalContributionId,
    "Unique time-sortable identifier for an append-only savings goal contribution (UUID v7)."
);

define_id_v7!(
    AccountBalanceSnapshotId,
    "Unique time-sortable identifier for a periodic account balance snapshot (UUID v7)."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v4_ids_generate_valid_uuid_v4() {
        let family_id = FamilyId::new_v4();
        assert_eq!(family_id.as_uuid().get_version_num(), 4);

        let member_id = MemberId::new_v4();
        assert_eq!(member_id.as_uuid().get_version_num(), 4);

        let invite_id = InviteTokenId::new_v4();
        assert_eq!(invite_id.as_uuid().get_version_num(), 4);
    }

    #[test]
    fn v7_ids_generate_valid_uuid_v7() {
        let tx_id = TransactionId::now_v7();
        assert_eq!(tx_id.as_uuid().get_version_num(), 7);

        let payment_id = LoanPaymentId::now_v7();
        assert_eq!(payment_id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn id_string_parsing_and_display() {
        let raw = Uuid::new_v4();
        let family_id = FamilyId::from_uuid(raw);

        assert_eq!(family_id.to_string(), raw.to_string());
        assert_eq!(FamilyId::from_str(&raw.to_string()).unwrap(), family_id);
    }

    #[test]
    fn id_serde_transparent() {
        let raw = Uuid::new_v4();
        let family_id = FamilyId::from_uuid(raw);

        let json = serde_json::to_string(&family_id).unwrap();
        assert_eq!(json, format!("\"{}\"", raw));

        let deserialized: FamilyId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, family_id);
    }
}

