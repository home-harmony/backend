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
| `DomainAction` | `domain::value_objects::DomainAction` | Unified policy table key: `Permission(Permission)`, `ViewFamilyWorkspace` (any active member), or `ManageFamily` (`Owner` only) evaluated via exhaustive `action_policy` matrix |
| `FamilyManagement` | `Role::can_manage_family()` / `Role::assert_family_management()` | Exclusive **Owner-only** governance over `Family` membership (updating profile, inviting members, changing roles, removing members). |
| `DomainError` | `domain::errors::DomainError` | Typed domain errors (`InsufficientRole`, `FamilyNotFound`, `FamilyDeleted`, `DuplicateOwner`, `CannotRemoveOwner`, `MemberAlreadyExists`, `MemberNotFound`, `UserNotInFamily`, `InviteTokenNotFound`, `TokenExpired`, `TokenAlreadyUsed`, `InvariantViolation`) |

---

## Rules

| ID | Status | Origin | Given State / Role & Custom Permissions | Action / Input | Expected Result / Next State |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `FM-01` | `Approved` | `doc:documentation/domain/bounded-contexts.md` | `Role::Owner`, `custom = []` | `has_permission(&custom, any Permission)` | `true` for each of the 8 ledger/view flags |
| `FM-02` | `Approved` | `doc:documentation/domain/bounded-contexts.md` | `Role::Member`, `custom = []` | `has_permission(&custom, any Permission)` | `true` for each of the 8 ledger/view flags (`ManageFamily` stays denied) |
| `FM-03` | `Approved` | `doc:documentation/domain/bounded-contexts.md` | `Role::Child`, `custom = [ViewLoans]` | `has_permission(&custom, Permission::ViewLoans)` | `false` (`custom` ignored for fixed roles) |
| `FM-04` | `Approved` | `doc:documentation/domain/bounded-contexts.md` | `Role::Child`, `custom = []` | `has_permission(&custom, Permission::RecordOwnTransactions)` | `true` |
| `FM-05` | `Approved` | `doc:documentation/domain/bounded-contexts.md` | `Role::Other`, `custom = [ViewLoans]` | `has_permission(&custom, Permission::ViewLoans)` | `true` |
| `FM-06` | `Approved` | `doc:documentation/domain/bounded-contexts.md` | `Role::Child`, `custom = [ViewLoans]` | `assert_debt_access(&custom)` | `Err(DomainError::InsufficientRole { role: Role::Child })` |
| `FM-07` | `Approved` | `human` | `Role::Other`, `custom = [ViewLoans]` | `assert_debt_access(&custom)` | `Ok(())` |
| `FM-08` | `Approved` | `human` | `Role::Owner`, `custom = []` | `assert_family_management()` | `Ok(())` |
| `FM-09` | `Approved` | `human` | `Role::Other`, `custom = []` (missing `ViewLoans`) | `assert_debt_access(&custom)` | `Err(DomainError::InsufficientRole { role: Role::Other })` |
| `FM-10` | `Approved` | `doc:documentation/domain/bounded-contexts.md` | `Role::Other`, `custom = [ViewLoans]` | `has_permission(&custom, Permission::ViewBudget)` | `false` (absent flag denied) |
| `FM-11` | `Approved` | `human` | `Role::Owner`, `custom = []` | `assert_debt_access(&custom)` | `Ok(())` |
| `FM-12` | `Approved` | `human` | `Role::Member`, `custom = []` | `assert_debt_access(&custom)` | `Ok(())` |
| `FM-13` | `Approved` | `human` | `Role::Member`, `Role::Child`, or `Role::Other` (even with all 8 flags) | `assert_family_management()` | `Err(DomainError::InsufficientRole { role })` |
| `FM-14` | `Approved` | `doc:documentation/domain/bounded-contexts.md` | `Role::Child`, `custom = []` | `has_permission(&custom, Permission::ManageOwnAccounts)` | `true` |
| `FM-15` | `Approved` | `human` | Active `Family` with unused unexpired invite token and `user_id` already active | `accept_invite_at(token, user_id, name, now)` | `Err(DomainError::MemberAlreadyExists { user_id })`; token remains `is_used() == false` |
| `FM-16` | `Approved` | `human` | Active `Family` with soft-deleted `FamilyMember` for `user_id` and valid invite | `accept_invite_at(token, user_id, new_name, now)` | Reactivates existing member preserving `id()`, clears `deleted_at`, updates role/permissions/relationship, emits `MemberJoined` |
| `FM-17` | `Approved` | `human` | Soft-deleted `Family` (`is_deleted() == true`) | `create_invite_at` / `accept_invite_at` / `change_member_role_at` / `remove_member_at` | `Err(DomainError::FamilyDeleted)` |
| `FM-18` | `Approved` | `human` | Unused invite token where `now >= invite.expires_at()` | `accept_invite_at(token, user_id, name, now)` | `Err(DomainError::TokenExpired)` |
| `FM-19` | `Approved` | `human` | Token ID absent from `family.invite_tokens()` | `accept_invite_at(token, user_id, name, now)` | `Err(DomainError::InviteTokenNotFound)` |
| `FM-20` | `Approved` | `human` | Owner targets active member with `Role::Other` and `[ViewBudget, ViewBudget]` | `change_member_role_at(..., Role::Other, perms, now)` | Deduplicates permissions to `[ViewBudget]`, updates role/permissions, emits `MemberRoleChanged` carrying updated `permissions` |
| `FM-21` | `Approved` | `human` | Owner targets active member with identical `(role, permissions)` | `change_member_role_at(..., same_role, same_perms, now)` | `Ok(vec![])` (no-op emits zero events) |
| `FM-22` | `Approved` | `human` | Owner attempts to demote self or promote another member to `Role::Owner` | `change_member_role_at(...)` / `remove_member_at(...)` | `Err(DomainError::CannotRemoveOwner)` on Owner demote/remove; `Err(DomainError::DuplicateOwner)` on promotion |
| `FM-23` | `Approved` | `human` | Active `Family` with invite token where `is_used() == true` | `accept_invite_at(token, user_id, name, now)` | `Err(DomainError::TokenAlreadyUsed)` |
| `FM-24` | `Approved` | `human` | Active `Family` where `requester_user_id` is absent or soft-deleted | `create_invite_at` / `change_member_role_at` / `remove_member_at` / `update_profile` | `Err(DomainError::UserNotInFamily { user_id })` |
| `FM-25` | `Approved` | `human` | Active `Family` where `target_member_id` is absent or soft-deleted | `change_member_role_at` / `remove_member_at` | `Err(DomainError::MemberNotFound { member_id })` |
| `FM-26` | `Approved` | `human` | Active `Family` with `Owner` caller | `create_invite_at(..., Role::Owner, ...)` or `create_invite_at(..., Role::Member \| Child, non_empty_perms, ...)` | `Err(DomainError::DuplicateOwner)` for `Role::Owner`; `Err(DomainError::InvariantViolation { .. })` for custom permissions on non-`Other` |
| `FM-27` | `Approved` | `human` | Active vs. soft-deleted `Family` with `Owner` or non-`Owner` caller | `update_profile(requester_user_id, new_name, new_home_currency)` | Updates `name` and `home_currency` for active `Owner`; `Err(DomainError::InsufficientRole { role })` for non-`Owner`; `Err(DomainError::FamilyDeleted)` if deleted |
| `FM-28` | `Approved` | `human` | Active `Family` with unused unexpired invite token under two concurrent acceptance attempts | Two concurrent `POST /families/{id}/members` (or repository `update`) calls with the same `token` | Exactly one caller joins (`201` / `Ok`) and marks `used = true`; the second receives `409 TOKEN_ALREADY_USED` (`TokenAlreadyUsed`) |
| `FM-29` | `Approved` | `human` | Active `Family` with concurrent profile update and member removal (`remove_member`) | Concurrent `PUT /families/{id}` and `DELETE /families/{id}/members/{mid}` | Both serialize cleanly without resurrecting the soft-deleted member (`deleted_at` remains set) or reverting the profile update |
| `FM-30` | `Draft` | `human` | Active `Family`, valid `Owner`, non-Owner role, bounded `InviteTtl` | `create_invite_at(...)` | Populates `invite.id(): InviteId` and `invite.token_hash(): &InviteTokenHash` (SHA-256 of `invite.token()`); emits `DomainEvent::MemberInvited` carrying `invite_id` and `invite_token_hash` (never raw `InviteTokenId`) |
| `FM-31` | `Draft` | `human` | Active `Family` with `InviteToken` reconstituted from hashed DB row (`id`, `token_hash`, `used: false`, unexpired) | `accept_invite_at(presented_token, user_id, name, now)` | Verifies `InviteTokenHash::from_token_id(presented_token) == *invite.token_hash()`, marks invite used, adds/reactivates member, emits `MemberJoined` |
| `FM-32` | `Draft` | `human` | Sprint 3 Ubiquitous Language rename (`Role::Other` -> `Role::Guest`, wire `"guest"`) | `Role::try_from`, serde serialize/deserialize, `action_policy`, `Family::create_invite_at`, `Family::change_member_role_at` | `"guest"` round-trips to `Role::Guest` (`"other"` rejected); custom `Permission` flags apply strictly to `Role::Guest` (`FM-05`, `FM-07`, `FM-09`, `FM-10`, `FM-13`, `FM-20` updated simultaneously in `Slice B0`) |

---

## Open Questions

*(None)*

---

## Findings

| # | Location | Code Says | Doc / Spec Says | Impact |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `backend/domain/src/value_objects/role.rs` | Unified all checks in exhaustive 4-column `const fn action_policy` matrix; `assert_debt_access(&custom)` checks `Permission::ViewLoans` for `Role::Other` | `FM-07` / `FM-09` (`Origin: human`, Option B): `Role::Other` requires explicit `Permission` flags on debt, report, and account checks | Resolved on 2026-11-20 |
| 2 | `backend/domain/src/entities/family.rs` | Added `FamilyDeleted`, `MemberAlreadyExists`, `InviteTokenNotFound`, soft-deleted reactivation (`2.1-A`), `change_member_role` permissions + no-op (`2.2-A`), and `*_at` deterministic clock methods (`2.3A-1`) | Aligned in `FM-15`–`FM-22` (`Status: Approved`) | Covered by `domain/tests/fm_family_rbac.rs` |
