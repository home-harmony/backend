//! `Role` — family member role value object.
//!
//! Roles determine what actions a family member can perform.
//! Authorization is enforced at the **API layer** — Lambdas check the role
//! extracted from the Cognito JWT before processing any request.
//!
//! # Role Hierarchy
//!
//! ```text
//! Owner  — full control: manage members, view all reports, all financial actions
//! Member — adult member: record transactions, manage own cards, view all finances
//! Other  — extended family (Grandma, etc.): same as Member by default
//! Child  — restricted: own transactions only; BLOCKED from loans, debt plans, reports
//! ```

use crate::errors::DomainError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Family member role controlling permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Member,
    Child,
    Other,
}

impl Role {
    /// Returns `true` if this role is allowed to view and manage loans/debt plans.
    ///
    /// RULE (GEMINI.md §16): Children must be blocked from loans, debt plans,
    /// and family-level reports.
    pub fn can_access_debt_features(&self) -> bool {
        matches!(self, Role::Owner | Role::Member | Role::Other)
    }

    /// Returns `true` if this role can manage family membership (invite, remove, change roles).
    pub fn can_manage_family(&self) -> bool {
        matches!(self, Role::Owner)
    }

    /// Returns `true` if this role can view family-level financial reports.
    pub fn can_view_family_reports(&self) -> bool {
        matches!(self, Role::Owner | Role::Member | Role::Other)
    }

    /// Returns `true` if this role can register and modify payment accounts.
    pub fn can_manage_accounts(&self) -> bool {
        matches!(self, Role::Owner | Role::Member | Role::Other)
    }

    /// Asserts that this role has access to debt features, returning an error otherwise.
    pub fn assert_debt_access(&self) -> Result<(), DomainError> {
        if self.can_access_debt_features() {
            Ok(())
        } else {
            Err(DomainError::InsufficientRole {
                role: self.to_string(),
            })
        }
    }

    /// Asserts that this role can manage the family, returning an error otherwise.
    pub fn assert_family_management(&self) -> Result<(), DomainError> {
        if self.can_manage_family() {
            Ok(())
        } else {
            Err(DomainError::InsufficientRole {
                role: self.to_string(),
            })
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Owner => write!(f, "owner"),
            Role::Member => write!(f, "member"),
            Role::Child => write!(f, "child"),
            Role::Other => write!(f, "other"),
        }
    }
}

impl TryFrom<&str> for Role {
    type Error = DomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "owner" => Ok(Role::Owner),
            "member" => Ok(Role::Member),
            "child" => Ok(Role::Child),
            "other" => Ok(Role::Other),
            _ => Err(DomainError::InvariantViolation {
                message: format!("Unknown role: '{}'", value),
            }),
        }
    }
}

impl TryFrom<String> for Role {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Role::try_from(value.as_str())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_cannot_access_debt_features() {
        assert!(!Role::Child.can_access_debt_features());
        assert!(Role::Child.assert_debt_access().is_err());
    }

    #[test]
    fn owner_member_other_can_access_debt() {
        assert!(Role::Owner.can_access_debt_features());
        assert!(Role::Member.can_access_debt_features());
        assert!(Role::Other.can_access_debt_features());
    }

    #[test]
    fn only_owner_manages_family() {
        assert!(Role::Owner.can_manage_family());
        assert!(!Role::Member.can_manage_family());
        assert!(!Role::Child.can_manage_family());
        assert!(!Role::Other.can_manage_family());
    }

    #[test]
    fn round_trip_from_str() {
        assert_eq!(Role::try_from("owner").unwrap(), Role::Owner);
        assert_eq!(Role::try_from("member").unwrap(), Role::Member);
        assert_eq!(Role::try_from("child").unwrap(), Role::Child);
        assert_eq!(Role::try_from("other").unwrap(), Role::Other);
    }

    #[test]
    fn unknown_role_errors() {
        assert!(Role::try_from("admin").is_err());
    }

    #[test]
    fn display_is_lowercase() {
        assert_eq!(Role::Owner.to_string(), "owner");
        assert_eq!(Role::Child.to_string(), "child");
    }
}
