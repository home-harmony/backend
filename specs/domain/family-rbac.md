# FM — Family Workspace & RBAC Permissions

- **Prefix**: `FM`
- **Layer**: `domain`
- **Unit under test**: `domain::value_objects::{Role, Permission, DomainAction}` and `domain::entities::{Family, FamilyMember}`
- **Test seam**: `Layer 1 (Pure Domain)`
- **Format**: `T` (Case Table)
- **Test target**: `domain/tests/fm_family_rbac.rs`

---

## Vocabulary

| Term | Meaning / API Symbol | Concrete Representation |
| :--- | :--- | :--- |
| `Role` | `domain::value_objects::Role` | `Owner`, `Member`, `Child`, `Other` |
| `Permission` | `domain::value_objects::Permission` | The 8 **ledger/view capability flags** (`RecordOwnTransactions`, `RecordAnyTransaction`, `ViewAllTransactions`, `ViewAllAccounts`, `ManageOwnAccounts`, `ViewLoans`, `ViewBudget`, `ViewReports`). Does **not** include family membership administration. |
| `DomainAction` | `domain::value_objects::DomainAction` | Unified policy table key: `Permission(Permission)` or `ManageFamily` evaluated via exhaustive `action_policy` matrix |
| `FamilyManagement` | `Role::can_manage_family()` / `Role::assert_family_management()` | Exclusive **Owner-only** governance over `Family` membership (inviting members, changing roles, removing members). |
| `InsufficientRole` | `domain::errors::DomainError::InsufficientRole` | Returned when role or custom permissions disallow the requested action |

---

## Rules

| ID | Status | Origin | Given Role & Custom Permissions | Action / Input | Expected Result |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `FM-01` | `Draft` | `doc:documentation/domain/ubiquitous-language.md` | `Role::Owner`, `custom = []` | `has_permission(&custom, any Permission)` (any of the 8 ledger/view flags) | `true` (all 8 ledger/view flags allowed) |
| `FM-02` | `Draft` | `doc:documentation/domain/ubiquitous-language.md` | `Role::Member`, `custom = []` | `has_permission(&custom, any Permission)` (any of the 8 ledger/view flags) | `true` (`Member` has all 8 ledger/view flags, **not** `ManageFamily`) |
| `FM-03` | `Draft` | `observed` | `Role::Child`, `custom = [ViewLoans]` | `has_permission(&custom, Permission::ViewLoans)` | `false` (`custom` ignored for fixed roles) |
| `FM-04` | `Draft` | `observed` | `Role::Child`, `custom = []` | `has_permission(&custom, Permission::RecordOwnTransactions)` | `true` (only `RecordOwnTransactions` and `ManageOwnAccounts` allowed) |
| `FM-05` | `Draft` | `doc:documentation/domain/ubiquitous-language.md` | `Role::Other`, `custom = [ViewLoans]` | `has_permission(&custom, Permission::ViewLoans)` | `true`; `false` for permissions absent from `custom` |
| `FM-06` | `Draft` | `doc:documentation/domain/bounded-contexts.md` | `Role::Child`, `custom = [ViewLoans]` | `assert_debt_access(&custom)` | `Err(DomainError::InsufficientRole { role: Role::Child })` |
| `FM-07` | `Draft` | `human` | `Role::Owner` / `Role::Member`, or `Role::Other` with `custom = [ViewLoans]` | `assert_debt_access(&custom)` | `Ok(())` |
| `FM-08` | `Draft` | `human` | `Role::Owner` vs `Role::Member` / `Role::Other` / `Role::Child` | `assert_family_management()` | `Ok(())` strictly for `Role::Owner`; `Err(DomainError::InsufficientRole)` for `Member`, `Other`, and `Child` |
| `FM-09` | `Draft` | `human` | `Role::Other`, `custom = []` (or missing `Permission::ViewLoans`) | `assert_debt_access(&custom)` | `Err(DomainError::InsufficientRole { role: Role::Other })` |

---

## Open Questions

*(None)*

---

## Findings

| # | Location | Code Says | Doc / Spec Says | Impact |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `backend/domain/src/value_objects/role.rs` | Unified all checks in exhaustive 4-column `const fn action_policy` matrix; `assert_debt_access(&custom)` checks `Permission::ViewLoans` for `Role::Other` | `FM-07` / `FM-09` (`Origin: human`, Option B): `Role::Other` requires explicit `Permission` flags on debt, report, and account checks | Resolved on 2026-11-20 |
