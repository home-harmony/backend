//! `Role`, `Permission`, and compile-time `DomainAction` authorization matrix.
//!
//! All authorization checks evaluate through a single exhaustive 4-column matrix
//! in [`action_policy`], giving O(1) lookup and compile-time safety whenever a new
//! `Permission`, `Role`, or `DomainAction` is added.
//!
//! # Authorization Matrix (4 Roles × 13 Actions)
//!
//! | Action / Permission      | `Owner` | `Member` | `Child` | `Guest`  |
//! | :----------------------- | :-----: | :------: | :-----: | :------: |
//! | `RecordOwnTransactions`  | Allow   | Allow    | Allow   | Custom*  |
//! | `ManageOwnAccounts`      | Allow   | Allow    | Allow   | Custom*  |
//! | `RecordAnyTransaction`   | Allow   | Allow    | Deny    | Custom*  |
//! | `ViewAllTransactions`    | Allow   | Allow    | Deny    | Custom*  |
//! | `ViewAllAccounts`        | Allow   | Allow    | Deny    | Custom*  |
//! | `ViewLoans`              | Allow   | Allow    | Deny    | Custom*  |
//! | `ViewBudget`             | Allow   | Allow    | Deny    | Custom*  |
//! | `ViewReports`            | Allow   | Allow    | Deny    | Custom*  |
//! | `ViewFamilyWorkspace`    | Allow   | Allow    | Allow   | Allow    |
//! | `ManageFamily`           | Allow   | Deny     | Deny    | Deny     |
//! | `AccessAccounts`         | Allow   | Allow    | Allow   | Custom*  |
//! | `AccessLedger`           | Allow   | Allow    | Allow   | Custom*  |
//! | `RecordTransactions`     | Allow   | Allow    | Allow   | Custom*  |
//!
//! \* **`Custom`**: allowed only when the `Permission` flag (or any matching capability
//! flag for composite actions) is present in `family_members.permissions`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::errors::DomainError;

/// Granular capability flags for family ledger, account, budget, loan, and reporting access.
///
/// Predefined roles (`Owner`, `Member`, `Child`) have fixed rules in [`action_policy`].
/// For `Role::Guest` members, the Family `Owner` grants a subset of these flags
/// at invite time (stored as JSONB in `family_members.permissions`).
///
/// Note: Family membership administration (`ManageFamily`) is intentionally **not** a
/// `Permission` flag—it is strictly reserved for `Role::Owner`.
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

/// Family workspace role assigned to each [`FamilyMember`](crate::entities::FamilyMember).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Creator/administrator: all 8 ledger/view permissions plus exclusive `ManageFamily`.
    Owner,
    /// Adult household member: all 8 ledger/view permissions, no `ManageFamily`.
    Member,
    /// Restricted member: own transactions and own accounts only; blocked from loans/budgets/reports.
    Child,
    /// Guest or extended household participant: governed strictly by Owner-assigned `custom_permissions`.
    Guest,
}

/// Unified domain action key evaluated against the central [`action_policy`] matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DomainAction {
    /// Any of the 8 granular ledger/view [`Permission`] flags.
    Permission(Permission),
    /// Reading family workspace profile and active member roster (any active member).
    ViewFamilyWorkspace,
    /// Family membership governance: inviting members, changing roles, and removing members.
    ManageFamily,
    /// Reading payment accounts or snapshots (`ViewAllAccounts` or `ManageOwnAccounts` for `Guest`).
    AccessAccounts,
    /// Reading categories or ledger transactions (`ViewAllTransactions`, `RecordOwnTransactions`, or `RecordAnyTransaction` for `Guest`).
    AccessLedger,
    /// Recording, amending, or deleting transactions (`RecordOwnTransactions` or `RecordAnyTransaction` for `Guest`).
    RecordTransactions,
}

/// Outcome of looking up a `(DomainAction, Role)` pair in [`action_policy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessRule {
    /// Action is unconditionally permitted for this role.
    Allow,
    /// Action is unconditionally forbidden for this role, regardless of `custom_permissions`.
    Deny,
    /// Action is permitted only if `custom_permissions` contains the target `Permission`.
    Custom,
}

impl Permission {
    /// Returns all 8 defined ledger/view permission flags.
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

impl Role {
    /// Returns the fixed [`Permission`] set for predefined roles (`Owner`, `Member`, `Child`),
    /// or an empty `Vec` for `Role::Guest` (which relies on per-member `custom_permissions`).
    pub fn default_permissions(&self) -> Vec<Permission> {
        Permission::all()
            .into_iter()
            .filter(|&perm| self.has_permission(&[], perm))
            .collect()
    }

    /// Evaluates whether this role and `custom_permissions` grant `perm`.
    ///
    /// For `Owner`, `Member`, and `Child`, `custom_permissions` is ignored and the fixed
    /// matrix rules apply. For `Guest`, `custom_permissions` must contain `perm`.
    pub fn has_permission(&self, custom_permissions: &[Permission], perm: Permission) -> bool {
        self.can_perform(custom_permissions, DomainAction::Permission(perm))
    }

    /// Evaluates any [`DomainAction`] in O(1) against the exhaustive [`action_policy`] matrix.
    pub fn can_perform(&self, custom_permissions: &[Permission], action: DomainAction) -> bool {
        match (action_policy(action, *self), action) {
            (AccessRule::Allow, _) => true,
            (AccessRule::Deny, _) => false,
            (AccessRule::Custom, DomainAction::Permission(perm)) => {
                custom_permissions.contains(&perm)
            }
            (AccessRule::Custom, DomainAction::AccessAccounts) => has_any_permission(
                custom_permissions,
                &[Permission::ViewAllAccounts, Permission::ManageOwnAccounts],
            ),
            (AccessRule::Custom, DomainAction::AccessLedger) => has_any_permission(
                custom_permissions,
                &[
                    Permission::ViewAllTransactions,
                    Permission::RecordOwnTransactions,
                    Permission::RecordAnyTransaction,
                ],
            ),
            (AccessRule::Custom, DomainAction::RecordTransactions) => has_any_permission(
                custom_permissions,
                &[
                    Permission::RecordOwnTransactions,
                    Permission::RecordAnyTransaction,
                ],
            ),
            // Non-permission governance/workspace actions cannot be delegated via custom flags.
            (
                AccessRule::Custom,
                DomainAction::ManageFamily | DomainAction::ViewFamilyWorkspace,
            ) => false,
        }
    }

    /// Asserts that this role and `custom_permissions` may perform `action`, returning
    /// [`DomainError::InsufficientRole`] if denied.
    pub fn assert_can_perform(
        &self,
        custom_permissions: &[Permission],
        action: DomainAction,
    ) -> Result<(), DomainError> {
        if self.can_perform(custom_permissions, action) {
            return Ok(());
        }
        Err(DomainError::InsufficientRole { role: *self })
    }

    /// Returns `true` if this role and `custom_permissions` grant access to debt/loan features.
    ///
    /// For `Role::Guest`, requires [`Permission::ViewLoans`] in `custom_permissions`.
    pub fn can_access_debt_features(&self, custom_permissions: &[Permission]) -> bool {
        self.can_perform(
            custom_permissions,
            DomainAction::Permission(Permission::ViewLoans),
        )
    }

    /// Asserts access to debt/loan features, checking [`Permission::ViewLoans`] for `Role::Guest`.
    pub fn assert_debt_access(&self, custom_permissions: &[Permission]) -> Result<(), DomainError> {
        self.assert_can_perform(
            custom_permissions,
            DomainAction::Permission(Permission::ViewLoans),
        )
    }

    /// Returns `true` if this role can manage family membership (`Role::Owner` only).
    pub fn can_manage_family(&self) -> bool {
        self.can_perform(&[], DomainAction::ManageFamily)
    }

    /// Asserts that this role can manage family membership (`Role::Owner` only).
    pub fn assert_family_management(&self) -> Result<(), DomainError> {
        self.assert_can_perform(&[], DomainAction::ManageFamily)
    }

    /// Returns `true` if this role can read the family workspace and active member roster.
    pub fn can_view_family_workspace(&self) -> bool {
        self.can_perform(&[], DomainAction::ViewFamilyWorkspace)
    }

    /// Asserts that this role can read the family workspace and active member roster.
    pub fn assert_view_family_workspace(&self) -> Result<(), DomainError> {
        self.assert_can_perform(&[], DomainAction::ViewFamilyWorkspace)
    }

    /// Returns `true` if this role and `custom_permissions` allow viewing family reports.
    ///
    /// For `Role::Guest`, requires [`Permission::ViewReports`] in `custom_permissions`.
    pub fn can_view_family_reports(&self, custom_permissions: &[Permission]) -> bool {
        self.can_perform(
            custom_permissions,
            DomainAction::Permission(Permission::ViewReports),
        )
    }

    /// Returns `true` if this role and `custom_permissions` allow managing own payment accounts.
    ///
    /// For `Role::Guest`, requires [`Permission::ManageOwnAccounts`] in `custom_permissions`.
    pub fn can_manage_accounts(&self, custom_permissions: &[Permission]) -> bool {
        self.can_perform(
            custom_permissions,
            DomainAction::Permission(Permission::ManageOwnAccounts),
        )
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Owner => write!(f, "owner"),
            Role::Member => write!(f, "member"),
            Role::Child => write!(f, "child"),
            Role::Guest => write!(f, "guest"),
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
            "guest" => Ok(Role::Guest),
            _ => Err(DomainError::InvariantViolation {
                message: "Unknown role value".to_string(),
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

fn has_any_permission(granted: &[Permission], required_any: &[Permission]) -> bool {
    required_any.iter().any(|perm| granted.contains(perm))
}

/// Central compile-time authorization table mapping every `(DomainAction, Role)` pair.
///
/// Exhaustive `match` ensures adding a new `Permission`, `DomainAction`, or `Role` variant
/// triggers a compiler error until its policy row/column is explicitly specified here.
const fn action_policy(action: DomainAction, role: Role) -> AccessRule {
    use AccessRule::{Allow, Custom, Deny};
    use DomainAction::{
        AccessAccounts, AccessLedger, ManageFamily, Permission as P, RecordTransactions,
        ViewFamilyWorkspace,
    };
    use Permission::*;

    //                               Owner  Member Child  Guest
    let (owner, member, child, guest) = match action {
        P(RecordOwnTransactions) => (Allow, Allow, Allow, Custom),
        P(ManageOwnAccounts) => (Allow, Allow, Allow, Custom),
        P(RecordAnyTransaction) => (Allow, Allow, Deny, Custom),
        P(ViewAllTransactions) => (Allow, Allow, Deny, Custom),
        P(ViewAllAccounts) => (Allow, Allow, Deny, Custom),
        P(ViewLoans) => (Allow, Allow, Deny, Custom),
        P(ViewBudget) => (Allow, Allow, Deny, Custom),
        P(ViewReports) => (Allow, Allow, Deny, Custom),
        ViewFamilyWorkspace => (Allow, Allow, Allow, Allow),
        ManageFamily => (Allow, Deny, Deny, Deny),
        AccessAccounts => (Allow, Allow, Allow, Custom),
        AccessLedger => (Allow, Allow, Allow, Custom),
        RecordTransactions => (Allow, Allow, Allow, Custom),
    };

    match role {
        Role::Owner => owner,
        Role::Member => member,
        Role::Child => child,
        Role::Guest => guest,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_cannot_access_debt_features() {
        assert!(!Role::Child.can_access_debt_features(&[Permission::ViewLoans]));
        assert!(matches!(
            Role::Child.assert_debt_access(&[Permission::ViewLoans]),
            Err(DomainError::InsufficientRole { role: Role::Child })
        ));
    }

    #[test]
    fn debt_access_enforces_view_loans_for_guest_role() {
        assert!(Role::Owner.can_access_debt_features(&[]));
        assert!(Role::Member.can_access_debt_features(&[]));
        assert!(Role::Guest.can_access_debt_features(&[Permission::ViewLoans]));
        assert!(
            Role::Guest
                .assert_debt_access(&[Permission::ViewLoans])
                .is_ok()
        );

        assert!(!Role::Guest.can_access_debt_features(&[Permission::ViewBudget]));
        assert!(matches!(
            Role::Guest.assert_debt_access(&[]),
            Err(DomainError::InsufficientRole { role: Role::Guest })
        ));
    }

    #[test]
    fn only_owner_manages_family() {
        assert!(Role::Owner.can_manage_family());
        assert!(Role::Owner.assert_family_management().is_ok());
        assert!(!Role::Member.can_manage_family());
        assert!(!Role::Child.can_manage_family());
        assert!(!Role::Guest.can_manage_family());
    }

    #[test]
    fn reports_and_accounts_follow_policy_table() {
        assert!(Role::Member.can_view_family_reports(&[]));
        assert!(!Role::Child.can_view_family_reports(&[]));
        assert!(!Role::Guest.can_view_family_reports(&[]));
        assert!(Role::Guest.can_view_family_reports(&[Permission::ViewReports]));

        assert!(Role::Child.can_manage_accounts(&[]));
        assert!(!Role::Guest.can_manage_accounts(&[]));
        assert!(Role::Guest.can_manage_accounts(&[Permission::ManageOwnAccounts]));
    }

    #[test]
    fn manage_family_ignores_all_custom_permissions() {
        let all = Permission::all();
        assert!(!Role::Member.can_perform(&all, DomainAction::ManageFamily));
        assert!(!Role::Child.can_perform(&all, DomainAction::ManageFamily));
        assert!(!Role::Guest.can_perform(&all, DomainAction::ManageFamily));
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
        assert!(Role::Guest.default_permissions().is_empty());
    }

    #[test]
    fn has_permission_enforces_role_and_custom_flags() {
        assert!(Role::Owner.has_permission(&[], Permission::ViewLoans));
        assert!(Role::Member.has_permission(&[], Permission::ViewReports));
        assert!(Role::Child.has_permission(&[], Permission::RecordOwnTransactions));
        assert!(!Role::Child.has_permission(&[Permission::ViewLoans], Permission::ViewLoans));

        let custom = [Permission::ViewBudget, Permission::ViewAllTransactions];
        assert!(Role::Guest.has_permission(&custom, Permission::ViewBudget));
        assert!(!Role::Guest.has_permission(&custom, Permission::ViewLoans));
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
        assert_eq!(Role::try_from("guest").unwrap(), Role::Guest);
    }

    #[test]
    fn unknown_role_errors() {
        assert!(Role::try_from("admin").is_err());
        assert!(Role::try_from("other").is_err());
    }

    #[test]
    fn display_is_lowercase() {
        assert_eq!(Role::Owner.to_string(), "owner");
        assert_eq!(Role::Child.to_string(), "child");
    }
}
