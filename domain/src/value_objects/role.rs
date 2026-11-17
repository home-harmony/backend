//! `Role` and `Permission` — family member authorization value objects.
//!
//! Roles and permissions determine what actions a family member can perform.
//! Authorization is enforced at the **API layer** — Lambdas check the role and
//! permissions resolved for `(user_id, family_id)` in `family_members`.
//!
//! # Role Hierarchy
//!
//! ```text
//! Owner  — full control: all 8 permissions + family management
//! Member — adult member: all 8 permissions
//! Other  — extended family / caretaker: Owner-configured subset of the 8 permissions
//! Child  — restricted: RecordOwnTransactions + ManageOwnAccounts only;
//!          BLOCKED from loans, debt plans, budgets, and family reports
//! ```

use crate::errors::DomainError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Granular capability flags for family members.
///
/// Predefined roles (`Owner`, `Member`, `Child`) have fixed permission sets.
/// For `Role::Other` members, the Owner configures a subset of these flags
/// at invite time (stored as JSONB in `family_members.permissions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    RecordOwnTransactions,
    RecordAnyTransaction,
    ViewAllTransactions,
    ViewAllAccounts,
    ManageOwnAccounts,
    ViewLoans,
    ViewBudget,
    ViewReports,
}

impl Permission {
    /// Returns all 8 defined permission flags.
    pub const fn all() -> [Self; 8] {
        [
            Self::RecordOwnTransactions,
            Self::RecordAnyTransaction,
            Self::ViewAllTransactions,
            Self::ViewAllAccounts,
            Self::ManageOwnAccounts,
            Self::ViewLoans,
            Self::ViewBudget,
            Self::ViewReports,
        ]
    }
}

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
    /// Returns the fixed permission set for predefined roles, or an empty slice
    /// for `Role::Other` (which uses per-member custom permissions).
    pub fn default_permissions(&self) -> Vec<Permission> {
        match self {
            Role::Owner | Role::Member => Permission::all().to_vec(),
            Role::Child => vec![
                Permission::RecordOwnTransactions,
                Permission::ManageOwnAccounts,
            ],
            Role::Other => Vec::new(),
        }
    }

    /// Evaluates whether a member with this role and `custom_permissions` holds `perm`.
    ///
    /// For `Owner`, `Member`, and `Child`, `custom_permissions` is ignored and
    /// fixed role rules apply. For `Other`, `custom_permissions` is checked.
    pub fn has_permission(&self, custom_permissions: &[Permission], perm: Permission) -> bool {
        match self {
            Role::Owner | Role::Member => true,
            Role::Child => matches!(
                perm,
                Permission::RecordOwnTransactions | Permission::ManageOwnAccounts
            ),
            Role::Other => custom_permissions.contains(&perm),
        }
    }

    /// Returns `true` if this role is allowed to view and manage loans/debt plans.
    ///
    /// RULE (bounded-contexts.md / AGENTS.md): Children must be blocked from
    /// loans, debt plans, and family-level reports.
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
    fn default_permissions_match_role_contract() {
        assert_eq!(Role::Owner.default_permissions().len(), 8);
        assert_eq!(Role::Member.default_permissions().len(), 8);
        assert_eq!(
            Role::Child.default_permissions(),
            vec![
                Permission::RecordOwnTransactions,
                Permission::ManageOwnAccounts
            ]
        );
        assert!(Role::Other.default_permissions().is_empty());
    }

    #[test]
    fn has_permission_enforces_role_and_custom_flags() {
        assert!(Role::Owner.has_permission(&[], Permission::ViewLoans));
        assert!(Role::Member.has_permission(&[], Permission::ViewReports));
        assert!(Role::Child.has_permission(&[], Permission::RecordOwnTransactions));
        assert!(!Role::Child.has_permission(&[Permission::ViewLoans], Permission::ViewLoans));

        let custom = [Permission::ViewBudget, Permission::ViewAllTransactions];
        assert!(Role::Other.has_permission(&custom, Permission::ViewBudget));
        assert!(!Role::Other.has_permission(&custom, Permission::ViewLoans));
    }

    #[test]
    fn permission_serializes_snake_case() {
        let json = serde_json::to_string(&Permission::RecordOwnTransactions).unwrap();
        assert_eq!(json, "\"record_own_transactions\"");
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
