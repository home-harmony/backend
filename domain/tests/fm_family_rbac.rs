//! Blind static-table conformance tests for `backend/specs/domain/family-rbac.md` (`FM-01`..`FM-27`).

use chrono::{DateTime, Duration, Utc};

use domain::entities::{Family, FamilyMember, FamilyParams, InviteToken};
use domain::errors::DomainError;
use domain::events::DomainEvent;
use domain::value_objects::{
    CurrencyCode, MemberDisplayName, FamilyName, InviteTokenId, InviteTtl, MemberId, Permission,
    Relationship, Role, UserId,
};

#[derive(Clone, Copy)]
enum RolePolicyAction {
    HasPermission(Permission),
    AllEightPermissions,
    AllEightNoManageFamily,
    AssertDebtAccess,
    AssertFamilyManagement,
}

#[derive(Clone, Copy)]
enum RolePolicyExpected {
    Bool(bool),
    Ok,
    InsufficientRole(Role),
}

struct RolePolicyCase {
    id: &'static str,
    role: Role,
    permissions: &'static [Permission],
    action: RolePolicyAction,
    expected: RolePolicyExpected,
}

#[derive(Clone, Copy)]
enum FamilyGivenState {
    ActiveWithMember(Role),
    SoftDeletedMember,
    SoftDeletedFamily,
    ExpiredInviteAtBoundary,
    ExpiredInviteAfterBoundary,
    UnknownInvite,
    UsedInvite,
    CallerNotInFamily,
    UnknownTargetMember,
    SoftDeletedTargetMember,
    ActiveWithOtherMember,
}

#[derive(Clone, Copy)]
enum FamilyRuleAction {
    AcceptInviteAsExistingActive,
    AcceptInviteReactivate,
    CreateInviteOnDeletedFamily,
    AcceptInviteOnDeletedFamily,
    ChangeRoleOnDeletedFamily,
    RemoveMemberOnDeletedFamily,
    UpdateProfileOnDeletedFamily,
    AcceptExpiredOrMissingOrUsedInvite,
    CreateInviteOwnerRole,
    CreateInviteCustomPerms(Role),
    ChangeRoleToOtherDuplicatePerms,
    ChangeRoleSameRoleSamePerms,
    DemoteSelf,
    RemoveSelf,
    PromoteMemberToOwner,
    CallerCreateInvite,
    CallerChangeRole,
    CallerRemoveMember,
    CallerUpdateProfile,
    ChangeRoleMissingTarget,
    RemoveMemberMissingTarget,
    UpdateProfileAsOwner,
    UpdateProfileAsNonOwner,
}

#[derive(Debug, Clone, Copy)]
enum FamilyRuleExpected {
    MemberAlreadyExistsTokenUnused,
    ReactivatedPreservingMemberId,
    ErrFamilyDeleted,
    ErrTokenExpired,
    ErrInviteTokenNotFound,
    ErrTokenAlreadyUsed,
    DeduplicatedOtherPermissionsEvent,
    NoOpZeroEvents,
    ErrCannotRemoveOwner,
    ErrDuplicateOwner,
    ErrUserNotInFamily,
    ErrMemberNotFound,
    ErrInsufficientRole(Role),
    ErrInvariantViolation,
    ProfileUpdated,
}

struct FamilyRuleCase {
    id: &'static str,
    given: FamilyGivenState,
    permissions: &'static [Permission],
    action: FamilyRuleAction,
    expected: FamilyRuleExpected,
}

struct FamilyFixture {
    family: Family,
    owner_user_id: UserId,
    owner_member_id: MemberId,
    member_user_id: UserId,
    member_id: MemberId,
    invite: Option<InviteToken>,
}

const ALL_EIGHT_PERMS: &[Permission] = &[
    Permission::RecordOwnTransactions,
    Permission::RecordAnyTransaction,
    Permission::ViewAllTransactions,
    Permission::ViewAllAccounts,
    Permission::ManageOwnAccounts,
    Permission::ViewLoans,
    Permission::ViewBudget,
    Permission::ViewReports,
];

const ROLE_POLICY_TABLE: &[RolePolicyCase] = &[
    RolePolicyCase {
        id: "FM-01",
        role: Role::Owner,
        permissions: &[],
        action: RolePolicyAction::AllEightPermissions,
        expected: RolePolicyExpected::Bool(true),
    },
    RolePolicyCase {
        id: "FM-02",
        role: Role::Member,
        permissions: &[],
        action: RolePolicyAction::AllEightNoManageFamily,
        expected: RolePolicyExpected::Bool(true),
    },
    RolePolicyCase {
        id: "FM-03",
        role: Role::Child,
        permissions: &[Permission::ViewLoans],
        action: RolePolicyAction::HasPermission(Permission::ViewLoans),
        expected: RolePolicyExpected::Bool(false),
    },
    RolePolicyCase {
        id: "FM-04",
        role: Role::Child,
        permissions: &[],
        action: RolePolicyAction::HasPermission(Permission::RecordOwnTransactions),
        expected: RolePolicyExpected::Bool(true),
    },
    RolePolicyCase {
        id: "FM-05",
        role: Role::Other,
        permissions: &[Permission::ViewLoans],
        action: RolePolicyAction::HasPermission(Permission::ViewLoans),
        expected: RolePolicyExpected::Bool(true),
    },
    RolePolicyCase {
        id: "FM-06",
        role: Role::Child,
        permissions: &[Permission::ViewLoans],
        action: RolePolicyAction::AssertDebtAccess,
        expected: RolePolicyExpected::InsufficientRole(Role::Child),
    },
    RolePolicyCase {
        id: "FM-07",
        role: Role::Other,
        permissions: &[Permission::ViewLoans],
        action: RolePolicyAction::AssertDebtAccess,
        expected: RolePolicyExpected::Ok,
    },
    RolePolicyCase {
        id: "FM-08",
        role: Role::Owner,
        permissions: &[],
        action: RolePolicyAction::AssertFamilyManagement,
        expected: RolePolicyExpected::Ok,
    },
    RolePolicyCase {
        id: "FM-09",
        role: Role::Other,
        permissions: &[],
        action: RolePolicyAction::AssertDebtAccess,
        expected: RolePolicyExpected::InsufficientRole(Role::Other),
    },
    RolePolicyCase {
        id: "FM-10",
        role: Role::Other,
        permissions: &[Permission::ViewLoans],
        action: RolePolicyAction::HasPermission(Permission::ViewBudget),
        expected: RolePolicyExpected::Bool(false),
    },
    RolePolicyCase {
        id: "FM-11",
        role: Role::Owner,
        permissions: &[],
        action: RolePolicyAction::AssertDebtAccess,
        expected: RolePolicyExpected::Ok,
    },
    RolePolicyCase {
        id: "FM-12",
        role: Role::Member,
        permissions: &[],
        action: RolePolicyAction::AssertDebtAccess,
        expected: RolePolicyExpected::Ok,
    },
    RolePolicyCase {
        id: "FM-13",
        role: Role::Member,
        permissions: ALL_EIGHT_PERMS,
        action: RolePolicyAction::AssertFamilyManagement,
        expected: RolePolicyExpected::InsufficientRole(Role::Member),
    },
    RolePolicyCase {
        id: "FM-13",
        role: Role::Child,
        permissions: ALL_EIGHT_PERMS,
        action: RolePolicyAction::AssertFamilyManagement,
        expected: RolePolicyExpected::InsufficientRole(Role::Child),
    },
    RolePolicyCase {
        id: "FM-13",
        role: Role::Other,
        permissions: ALL_EIGHT_PERMS,
        action: RolePolicyAction::AssertFamilyManagement,
        expected: RolePolicyExpected::InsufficientRole(Role::Other),
    },
    RolePolicyCase {
        id: "FM-14",
        role: Role::Child,
        permissions: &[],
        action: RolePolicyAction::HasPermission(Permission::ManageOwnAccounts),
        expected: RolePolicyExpected::Bool(true),
    },
];

const FAMILY_RULE_TABLE: &[FamilyRuleCase] = &[
    FamilyRuleCase {
        id: "FM-15",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[],
        action: FamilyRuleAction::AcceptInviteAsExistingActive,
        expected: FamilyRuleExpected::MemberAlreadyExistsTokenUnused,
    },
    FamilyRuleCase {
        id: "FM-16",
        given: FamilyGivenState::SoftDeletedMember,
        permissions: &[Permission::ViewBudget],
        action: FamilyRuleAction::AcceptInviteReactivate,
        expected: FamilyRuleExpected::ReactivatedPreservingMemberId,
    },
    FamilyRuleCase {
        id: "FM-17",
        given: FamilyGivenState::SoftDeletedFamily,
        permissions: &[],
        action: FamilyRuleAction::CreateInviteOnDeletedFamily,
        expected: FamilyRuleExpected::ErrFamilyDeleted,
    },
    FamilyRuleCase {
        id: "FM-17",
        given: FamilyGivenState::SoftDeletedFamily,
        permissions: &[],
        action: FamilyRuleAction::AcceptInviteOnDeletedFamily,
        expected: FamilyRuleExpected::ErrFamilyDeleted,
    },
    FamilyRuleCase {
        id: "FM-17",
        given: FamilyGivenState::SoftDeletedFamily,
        permissions: &[],
        action: FamilyRuleAction::ChangeRoleOnDeletedFamily,
        expected: FamilyRuleExpected::ErrFamilyDeleted,
    },
    FamilyRuleCase {
        id: "FM-17",
        given: FamilyGivenState::SoftDeletedFamily,
        permissions: &[],
        action: FamilyRuleAction::RemoveMemberOnDeletedFamily,
        expected: FamilyRuleExpected::ErrFamilyDeleted,
    },
    FamilyRuleCase {
        id: "FM-18",
        given: FamilyGivenState::ExpiredInviteAtBoundary,
        permissions: &[],
        action: FamilyRuleAction::AcceptExpiredOrMissingOrUsedInvite,
        expected: FamilyRuleExpected::ErrTokenExpired,
    },
    FamilyRuleCase {
        id: "FM-18",
        given: FamilyGivenState::ExpiredInviteAfterBoundary,
        permissions: &[],
        action: FamilyRuleAction::AcceptExpiredOrMissingOrUsedInvite,
        expected: FamilyRuleExpected::ErrTokenExpired,
    },
    FamilyRuleCase {
        id: "FM-19",
        given: FamilyGivenState::UnknownInvite,
        permissions: &[],
        action: FamilyRuleAction::AcceptExpiredOrMissingOrUsedInvite,
        expected: FamilyRuleExpected::ErrInviteTokenNotFound,
    },
    FamilyRuleCase {
        id: "FM-20",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[Permission::ViewBudget, Permission::ViewBudget],
        action: FamilyRuleAction::ChangeRoleToOtherDuplicatePerms,
        expected: FamilyRuleExpected::DeduplicatedOtherPermissionsEvent,
    },
    FamilyRuleCase {
        id: "FM-21",
        given: FamilyGivenState::ActiveWithOtherMember,
        permissions: &[Permission::ViewBudget],
        action: FamilyRuleAction::ChangeRoleSameRoleSamePerms,
        expected: FamilyRuleExpected::NoOpZeroEvents,
    },
    FamilyRuleCase {
        id: "FM-22",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[],
        action: FamilyRuleAction::DemoteSelf,
        expected: FamilyRuleExpected::ErrCannotRemoveOwner,
    },
    FamilyRuleCase {
        id: "FM-22",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[],
        action: FamilyRuleAction::RemoveSelf,
        expected: FamilyRuleExpected::ErrCannotRemoveOwner,
    },
    FamilyRuleCase {
        id: "FM-22",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[],
        action: FamilyRuleAction::PromoteMemberToOwner,
        expected: FamilyRuleExpected::ErrDuplicateOwner,
    },
    FamilyRuleCase {
        id: "FM-23",
        given: FamilyGivenState::UsedInvite,
        permissions: &[],
        action: FamilyRuleAction::AcceptExpiredOrMissingOrUsedInvite,
        expected: FamilyRuleExpected::ErrTokenAlreadyUsed,
    },
    FamilyRuleCase {
        id: "FM-24",
        given: FamilyGivenState::CallerNotInFamily,
        permissions: &[],
        action: FamilyRuleAction::CallerCreateInvite,
        expected: FamilyRuleExpected::ErrUserNotInFamily,
    },
    FamilyRuleCase {
        id: "FM-24",
        given: FamilyGivenState::CallerNotInFamily,
        permissions: &[],
        action: FamilyRuleAction::CallerChangeRole,
        expected: FamilyRuleExpected::ErrUserNotInFamily,
    },
    FamilyRuleCase {
        id: "FM-24",
        given: FamilyGivenState::CallerNotInFamily,
        permissions: &[],
        action: FamilyRuleAction::CallerRemoveMember,
        expected: FamilyRuleExpected::ErrUserNotInFamily,
    },
    FamilyRuleCase {
        id: "FM-24",
        given: FamilyGivenState::CallerNotInFamily,
        permissions: &[],
        action: FamilyRuleAction::CallerUpdateProfile,
        expected: FamilyRuleExpected::ErrUserNotInFamily,
    },
    FamilyRuleCase {
        id: "FM-24",
        given: FamilyGivenState::SoftDeletedMember,
        permissions: &[],
        action: FamilyRuleAction::CallerCreateInvite,
        expected: FamilyRuleExpected::ErrUserNotInFamily,
    },
    FamilyRuleCase {
        id: "FM-24",
        given: FamilyGivenState::SoftDeletedMember,
        permissions: &[],
        action: FamilyRuleAction::CallerChangeRole,
        expected: FamilyRuleExpected::ErrUserNotInFamily,
    },
    FamilyRuleCase {
        id: "FM-24",
        given: FamilyGivenState::SoftDeletedMember,
        permissions: &[],
        action: FamilyRuleAction::CallerRemoveMember,
        expected: FamilyRuleExpected::ErrUserNotInFamily,
    },
    FamilyRuleCase {
        id: "FM-24",
        given: FamilyGivenState::SoftDeletedMember,
        permissions: &[],
        action: FamilyRuleAction::CallerUpdateProfile,
        expected: FamilyRuleExpected::ErrUserNotInFamily,
    },
    FamilyRuleCase {
        id: "FM-25",
        given: FamilyGivenState::UnknownTargetMember,
        permissions: &[],
        action: FamilyRuleAction::ChangeRoleMissingTarget,
        expected: FamilyRuleExpected::ErrMemberNotFound,
    },
    FamilyRuleCase {
        id: "FM-25",
        given: FamilyGivenState::SoftDeletedTargetMember,
        permissions: &[],
        action: FamilyRuleAction::ChangeRoleMissingTarget,
        expected: FamilyRuleExpected::ErrMemberNotFound,
    },
    FamilyRuleCase {
        id: "FM-25",
        given: FamilyGivenState::UnknownTargetMember,
        permissions: &[],
        action: FamilyRuleAction::RemoveMemberMissingTarget,
        expected: FamilyRuleExpected::ErrMemberNotFound,
    },
    FamilyRuleCase {
        id: "FM-25",
        given: FamilyGivenState::SoftDeletedTargetMember,
        permissions: &[],
        action: FamilyRuleAction::RemoveMemberMissingTarget,
        expected: FamilyRuleExpected::ErrMemberNotFound,
    },
    FamilyRuleCase {
        id: "FM-26",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[],
        action: FamilyRuleAction::CreateInviteOwnerRole,
        expected: FamilyRuleExpected::ErrDuplicateOwner,
    },
    FamilyRuleCase {
        id: "FM-26",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[Permission::ViewBudget],
        action: FamilyRuleAction::CreateInviteCustomPerms(Role::Member),
        expected: FamilyRuleExpected::ErrInvariantViolation,
    },
    FamilyRuleCase {
        id: "FM-26",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[Permission::ViewBudget],
        action: FamilyRuleAction::CreateInviteCustomPerms(Role::Child),
        expected: FamilyRuleExpected::ErrInvariantViolation,
    },
    FamilyRuleCase {
        id: "FM-27",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[],
        action: FamilyRuleAction::UpdateProfileAsOwner,
        expected: FamilyRuleExpected::ProfileUpdated,
    },
    FamilyRuleCase {
        id: "FM-27",
        given: FamilyGivenState::ActiveWithMember(Role::Member),
        permissions: &[],
        action: FamilyRuleAction::UpdateProfileAsNonOwner,
        expected: FamilyRuleExpected::ErrInsufficientRole(Role::Member),
    },
    FamilyRuleCase {
        id: "FM-27",
        given: FamilyGivenState::ActiveWithMember(Role::Child),
        permissions: &[],
        action: FamilyRuleAction::UpdateProfileAsNonOwner,
        expected: FamilyRuleExpected::ErrInsufficientRole(Role::Child),
    },
    FamilyRuleCase {
        id: "FM-27",
        given: FamilyGivenState::ActiveWithMember(Role::Other),
        permissions: &[],
        action: FamilyRuleAction::UpdateProfileAsNonOwner,
        expected: FamilyRuleExpected::ErrInsufficientRole(Role::Other),
    },
    FamilyRuleCase {
        id: "FM-27",
        given: FamilyGivenState::SoftDeletedFamily,
        permissions: &[],
        action: FamilyRuleAction::UpdateProfileOnDeletedFamily,
        expected: FamilyRuleExpected::ErrFamilyDeleted,
    },
];

#[test]
fn fm_role_policy_table_conforms() {
    for case in ROLE_POLICY_TABLE {
        assert_role_policy_case(case);
    }
}

#[test]
fn fm_family_aggregate_rule_table_conforms() {
    for case in FAMILY_RULE_TABLE {
        run_family_rule_case(case).unwrap();
    }
}

fn assert_role_policy_case(case: &RolePolicyCase) {
    match (case.action, case.expected) {
        (RolePolicyAction::HasPermission(perm), RolePolicyExpected::Bool(exp)) => {
            assert_eq!(
                case.role.has_permission(case.permissions, perm),
                exp,
                "{}",
                case.id
            );
        }
        (RolePolicyAction::AllEightPermissions, RolePolicyExpected::Bool(exp)) => {
            assert_all_permissions(case.id, case.role, case.permissions, exp);
        }
        (RolePolicyAction::AllEightNoManageFamily, RolePolicyExpected::Bool(exp)) => {
            assert_all_permissions(case.id, case.role, case.permissions, exp);
            assert!(!case.role.can_manage_family(), "{}", case.id);
        }
        (RolePolicyAction::AssertDebtAccess, expected) => {
            assert_role_result(case.id, case.role.assert_debt_access(case.permissions), expected);
        }
        (RolePolicyAction::AssertFamilyManagement, expected) => {
            assert_role_result(case.id, case.role.assert_family_management(), expected);
        }
        _ => panic!("invalid ROLE_POLICY_TABLE combination for {}", case.id),
    }
}

fn assert_all_permissions(id: &str, role: Role, permissions: &[Permission], expected: bool) {
    for perm in Permission::all() {
        assert_eq!(role.has_permission(permissions, perm), expected, "{id}");
    }
}

fn assert_role_result(id: &str, res: Result<(), DomainError>, expected: RolePolicyExpected) {
    match expected {
        RolePolicyExpected::Ok => assert!(res.is_ok(), "{id}: expected Ok, got {res:?}"),
        RolePolicyExpected::InsufficientRole(role) => {
            assert_eq!(res, Err(DomainError::InsufficientRole { role }), "{id}");
        }
        RolePolicyExpected::Bool(_) => panic!("unexpected Bool expectation for {id}"),
    }
}

fn base_time() -> DateTime<Utc> {
    DateTime::UNIX_EPOCH + Duration::days(20_000)
}

fn build_fixture(given: FamilyGivenState) -> Result<FamilyFixture, DomainError> {
    let owner_user_id = UserId::new_v4();
    let (mut family, owner_member_id) = create_test_family(owner_user_id)?;
    let (member_role, member_perms) = match given {
        FamilyGivenState::ActiveWithOtherMember => (Role::Other, vec![Permission::ViewBudget]),
        FamilyGivenState::ActiveWithMember(role) => (role, Vec::new()),
        _ => (Role::Member, Vec::new()),
    };
    let member_user_id = UserId::new_v4();
    let (member_id, token) =
        add_fixture_member(&mut family, owner_user_id, member_user_id, member_role, member_perms)?;
    let invite = prepare_given_state(&mut family, owner_user_id, member_id, given, token)?;
    Ok(FamilyFixture {
        family,
        owner_user_id,
        owner_member_id,
        member_user_id,
        member_id,
        invite,
    })
}

fn create_test_family(owner_user_id: UserId) -> Result<(Family, MemberId), DomainError> {
    let (family, _) = Family::create_at(
        FamilyName::try_new("Test Family")?,
        CurrencyCode::try_new("USD")?,
        owner_user_id,
        MemberDisplayName::try_new("Alice")?,
        base_time(),
    )?;
    let owner_member_id =
        family
            .members()
            .first()
            .map(FamilyMember::id)
            .ok_or(DomainError::InvariantViolation {
                message: "missing owner".to_owned(),
            })?;
    Ok((family, owner_member_id))
}

fn add_fixture_member(
    family: &mut Family,
    owner_user_id: UserId,
    member_user_id: UserId,
    role: Role,
    permissions: Vec<Permission>,
) -> Result<(MemberId, InviteToken), DomainError> {
    let ttl = InviteTtl::from_days(7)?;
    let (token, _) =
        family.create_invite_at(owner_user_id, role, permissions, None, ttl, base_time())?;
    let (member, _) = family.accept_invite_at(
        token.token(),
        member_user_id,
        MemberDisplayName::try_new("Bob")?,
        base_time() + Duration::hours(1),
    )?;
    Ok((member.id(), token))
}

fn prepare_given_state(
    family: &mut Family,
    owner_user_id: UserId,
    member_id: MemberId,
    given: FamilyGivenState,
    used_token: InviteToken,
) -> Result<Option<InviteToken>, DomainError> {
    let t2 = base_time() + Duration::hours(2);
    match given {
        FamilyGivenState::SoftDeletedMember | FamilyGivenState::SoftDeletedTargetMember => {
            family.remove_member_at(owner_user_id, member_id, t2)?;
            Ok(None)
        }
        FamilyGivenState::SoftDeletedFamily => {
            *family = mark_family_deleted(family, t2);
            Ok(None)
        }
        FamilyGivenState::ExpiredInviteAtBoundary
        | FamilyGivenState::ExpiredInviteAfterBoundary => {
            let ttl = InviteTtl::try_new(60)?;
            let (inv, _) =
                family.create_invite_at(owner_user_id, Role::Member, vec![], None, ttl, t2)?;
            Ok(Some(inv))
        }
        FamilyGivenState::UsedInvite => Ok(Some(used_token)),
        _ => Ok(None),
    }
}

fn mark_family_deleted(family: &Family, deleted_at: DateTime<Utc>) -> Family {
    Family::reconstitute(FamilyParams {
        id: family.id(),
        name: family.name().clone(),
        home_currency: family.home_currency().clone(),
        members: family.members().to_vec(),
        invite_tokens: family.invite_tokens().to_vec(),
        created_at: family.created_at(),
        deleted_at: Some(deleted_at),
    })
}

fn run_family_rule_case(case: &FamilyRuleCase) -> Result<(), DomainError> {
    let mut fx = build_fixture(case.given)?;
    let now = base_time() + Duration::hours(3);

    match (case.action, case.expected) {
        (
            FamilyRuleAction::AcceptInviteAsExistingActive,
            FamilyRuleExpected::MemberAlreadyExistsTokenUnused,
        ) => assert_existing_active_rejected(case.id, &mut fx, now),
        (
            FamilyRuleAction::AcceptInviteReactivate,
            FamilyRuleExpected::ReactivatedPreservingMemberId,
        ) => assert_reactivated(case.id, &mut fx, case.permissions, now),
        (
            FamilyRuleAction::ChangeRoleToOtherDuplicatePerms,
            FamilyRuleExpected::DeduplicatedOtherPermissionsEvent,
        ) => assert_dedup_role_change(case.id, &mut fx, case.permissions, now),
        (FamilyRuleAction::ChangeRoleSameRoleSamePerms, FamilyRuleExpected::NoOpZeroEvents) => {
            let events = fx.family.change_member_role_at(
                fx.owner_user_id,
                fx.member_id,
                Role::Other,
                case.permissions.to_vec(),
                now,
            )?;
            assert!(events.is_empty(), "{}", case.id);
            Ok(())
        }
        (FamilyRuleAction::UpdateProfileAsOwner, FamilyRuleExpected::ProfileUpdated) => {
            assert_profile_updated(case.id, &mut fx)
        }
        _ => assert_error_rule_case(case, &mut fx, now),
    }
}

fn assert_existing_active_rejected(
    id: &str,
    fx: &mut FamilyFixture,
    now: DateTime<Utc>,
) -> Result<(), DomainError> {
    let ttl = InviteTtl::from_days(7)?;
    let (token, _) =
        fx.family
            .create_invite_at(fx.owner_user_id, Role::Member, vec![], None, ttl, now)?;
    let name = MemberDisplayName::try_new("Dup")?;
    let err = fx
        .family
        .accept_invite_at(token.token(), fx.member_user_id, name, now)
        .err();
    let expected = Some(DomainError::MemberAlreadyExists {
        user_id: fx.member_user_id,
    });
    assert_eq!(err, expected, "{id}");
    let stored = fx
        .family
        .invite_tokens()
        .iter()
        .find(|t| t.token() == token.token());
    assert!(stored.is_some_and(|t| !t.is_used()), "{id}");
    Ok(())
}

fn assert_reactivated(
    id: &str,
    fx: &mut FamilyFixture,
    perms: &[Permission],
    now: DateTime<Utc>,
) -> Result<(), DomainError> {
    let rel = Relationship::try_new("Grandma")?;
    let (token, _) = fx.family.create_invite_at(
        fx.owner_user_id,
        Role::Other,
        perms.to_vec(),
        Some(rel.clone()),
        InviteTtl::from_days(7)?,
        now,
    )?;
    let (member, events) = fx.family.accept_invite_at(
        token.token(),
        fx.member_user_id,
        MemberDisplayName::try_new("Grandma Alice")?,
        now + Duration::minutes(1),
    )?;
    assert_eq!(member.id(), fx.member_id, "{id}");
    assert!(member.is_active() && member.deleted_at().is_none(), "{id}");
    assert_eq!(member.relationship(), Some(&rel), "{id}");
    assert_eq!(member.permissions(), perms, "{id}");
    assert!(
        matches!(events.as_slice(), [DomainEvent::MemberJoined { .. }]),
        "{id}"
    );
    Ok(())
}

fn assert_dedup_role_change(
    id: &str,
    fx: &mut FamilyFixture,
    perms: &[Permission],
    now: DateTime<Utc>,
) -> Result<(), DomainError> {
    let events = fx.family.change_member_role_at(
        fx.owner_user_id,
        fx.member_id,
        Role::Other,
        perms.to_vec(),
        now,
    )?;
    let updated = fx.family.members().iter().find(|m| m.id() == fx.member_id);
    assert_eq!(
        updated.map(FamilyMember::permissions),
        Some([Permission::ViewBudget].as_slice()),
        "{id}"
    );
    assert!(
        matches!(
            events.as_slice(),
            [DomainEvent::MemberRoleChanged { new_role: Role::Other, permissions, .. }]
                if permissions == &[Permission::ViewBudget]
        ),
        "{id}"
    );
    Ok(())
}

fn assert_profile_updated(id: &str, fx: &mut FamilyFixture) -> Result<(), DomainError> {
    let new_name = FamilyName::try_new("Updated Family")?;
    let new_curr = CurrencyCode::try_new("EUR")?;
    fx.family
        .update_profile(fx.owner_user_id, new_name.clone(), new_curr.clone())?;
    assert_eq!(fx.family.name(), &new_name, "{id}");
    assert_eq!(fx.family.home_currency(), &new_curr, "{id}");
    Ok(())
}

fn assert_error_rule_case(
    case: &FamilyRuleCase,
    fx: &mut FamilyFixture,
    now: DateTime<Utc>,
) -> Result<(), DomainError> {
    let err = execute_error_action(case, fx, now)?;
    match (case.expected, err) {
        (FamilyRuleExpected::ErrFamilyDeleted, Some(DomainError::FamilyDeleted))
        | (FamilyRuleExpected::ErrTokenExpired, Some(DomainError::TokenExpired))
        | (FamilyRuleExpected::ErrInviteTokenNotFound, Some(DomainError::InviteTokenNotFound))
        | (FamilyRuleExpected::ErrTokenAlreadyUsed, Some(DomainError::TokenAlreadyUsed))
        | (FamilyRuleExpected::ErrCannotRemoveOwner, Some(DomainError::CannotRemoveOwner))
        | (FamilyRuleExpected::ErrDuplicateOwner, Some(DomainError::DuplicateOwner))
        | (FamilyRuleExpected::ErrUserNotInFamily, Some(DomainError::UserNotInFamily { .. }))
        | (FamilyRuleExpected::ErrMemberNotFound, Some(DomainError::MemberNotFound { .. }))
        | (
            FamilyRuleExpected::ErrInvariantViolation,
            Some(DomainError::InvariantViolation { .. }),
        ) => Ok(()),
        (
            FamilyRuleExpected::ErrInsufficientRole(r1),
            Some(DomainError::InsufficientRole { role: r2 }),
        ) if r1 == r2 => Ok(()),
        (expected, actual) => panic!(
            "{}: unexpected result {actual:?} for {expected:?}",
            case.id as &str
        ),
    }
}

fn execute_error_action(
    case: &FamilyRuleCase,
    fx: &mut FamilyFixture,
    now: DateTime<Utc>,
) -> Result<Option<DomainError>, DomainError> {
    use FamilyRuleAction::*;
    let res = match case.action {
        CreateInviteOnDeletedFamily
        | AcceptInviteOnDeletedFamily
        | ChangeRoleOnDeletedFamily
        | RemoveMemberOnDeletedFamily
        | UpdateProfileOnDeletedFamily => execute_deleted_family_action(case.action, fx, now)?,
        AcceptExpiredOrMissingOrUsedInvite
        | CreateInviteOwnerRole
        | CreateInviteCustomPerms(_) => execute_invite_action(case, fx, now)?,
        CallerCreateInvite | CallerChangeRole | CallerRemoveMember | CallerUpdateProfile => {
            execute_unauthorized_caller_action(case.action, case.given, fx, now)?
        }
        DemoteSelf
        | RemoveSelf
        | PromoteMemberToOwner
        | ChangeRoleMissingTarget
        | RemoveMemberMissingTarget => {
            execute_governance_action(case.action, case.given, fx, now)?
        }
        UpdateProfileAsNonOwner => execute_non_owner_profile(fx)?,
        AcceptInviteAsExistingActive
        | AcceptInviteReactivate
        | ChangeRoleToOtherDuplicatePerms
        | ChangeRoleSameRoleSamePerms
        | UpdateProfileAsOwner => panic!("{}: success action reached error executor", case.id),
    };
    Ok(res.err())
}

fn execute_deleted_family_action(
    action: FamilyRuleAction,
    fx: &mut FamilyFixture,
    now: DateTime<Utc>,
) -> Result<Result<(), DomainError>, DomainError> {
    let ttl = InviteTtl::from_days(1)?;
    let bob = MemberDisplayName::try_new("Bob")?;
    Ok(match action {
        FamilyRuleAction::CreateInviteOnDeletedFamily => fx
            .family
            .create_invite_at(fx.owner_user_id, Role::Member, vec![], None, ttl, now)
            .map(|_| ()),
        FamilyRuleAction::AcceptInviteOnDeletedFamily => fx
            .family
            .accept_invite_at(InviteTokenId::new_v4(), UserId::new_v4(), bob, now)
            .map(|_| ()),
        FamilyRuleAction::ChangeRoleOnDeletedFamily => fx
            .family
            .change_member_role_at(fx.owner_user_id, fx.owner_member_id, Role::Member, vec![], now)
            .map(|_| ()),
        FamilyRuleAction::RemoveMemberOnDeletedFamily => fx
            .family
            .remove_member_at(fx.owner_user_id, fx.owner_member_id, now)
            .map(|_| ()),
        FamilyRuleAction::UpdateProfileOnDeletedFamily => fx.family.update_profile(
            fx.owner_user_id,
            FamilyName::try_new("Renamed")?,
            CurrencyCode::try_new("EUR")?,
        ),
        _ => unreachable!("non-deleted-family action"),
    })
}

fn execute_invite_action(
    case: &FamilyRuleCase,
    fx: &mut FamilyFixture,
    now: DateTime<Utc>,
) -> Result<Result<(), DomainError>, DomainError> {
    let ttl = InviteTtl::from_days(1)?;
    let bob = MemberDisplayName::try_new("Bob")?;
    Ok(match case.action {
        FamilyRuleAction::AcceptExpiredOrMissingOrUsedInvite => {
            accept_special_invite(case.given, fx, bob, now)
        }
        FamilyRuleAction::CreateInviteOwnerRole => fx
            .family
            .create_invite_at(fx.owner_user_id, Role::Owner, vec![], None, ttl, now)
            .map(|_| ()),
        FamilyRuleAction::CreateInviteCustomPerms(role) => {
            let perms = case.permissions.to_vec();
            fx.family
                .create_invite_at(fx.owner_user_id, role, perms, None, ttl, now)
                .map(|_| ())
        }
        _ => unreachable!("non-invite action"),
    })
}

fn execute_unauthorized_caller_action(
    action: FamilyRuleAction,
    given: FamilyGivenState,
    fx: &mut FamilyFixture,
    now: DateTime<Utc>,
) -> Result<Result<(), DomainError>, DomainError> {
    let caller = match given {
        FamilyGivenState::SoftDeletedMember => fx.member_user_id,
        _ => UserId::new_v4(),
    };
    let ttl = InviteTtl::from_days(1)?;
    Ok(match action {
        FamilyRuleAction::CallerCreateInvite => fx
            .family
            .create_invite_at(caller, Role::Member, vec![], None, ttl, now)
            .map(|_| ()),
        FamilyRuleAction::CallerChangeRole => fx
            .family
            .change_member_role_at(caller, fx.owner_member_id, Role::Child, vec![], now)
            .map(|_| ()),
        FamilyRuleAction::CallerRemoveMember => fx
            .family
            .remove_member_at(caller, fx.owner_member_id, now)
            .map(|_| ()),
        FamilyRuleAction::CallerUpdateProfile => fx.family.update_profile(
            caller,
            FamilyName::try_new("Renamed")?,
            CurrencyCode::try_new("EUR")?,
        ),
        _ => unreachable!("non-caller action"),
    })
}

fn execute_governance_action(
    action: FamilyRuleAction,
    given: FamilyGivenState,
    fx: &mut FamilyFixture,
    now: DateTime<Utc>,
) -> Result<Result<(), DomainError>, DomainError> {
    let target = match given {
        FamilyGivenState::SoftDeletedTargetMember => fx.member_id,
        _ => MemberId::new_v4(),
    };
    Ok(match action {
        FamilyRuleAction::DemoteSelf => fx
            .family
            .change_member_role_at(fx.owner_user_id, fx.owner_member_id, Role::Member, vec![], now)
            .map(|_| ()),
        FamilyRuleAction::RemoveSelf => fx
            .family
            .remove_member_at(fx.owner_user_id, fx.owner_member_id, now)
            .map(|_| ()),
        FamilyRuleAction::PromoteMemberToOwner => fx
            .family
            .change_member_role_at(fx.owner_user_id, fx.member_id, Role::Owner, vec![], now)
            .map(|_| ()),
        FamilyRuleAction::ChangeRoleMissingTarget => fx
            .family
            .change_member_role_at(fx.owner_user_id, target, Role::Child, vec![], now)
            .map(|_| ()),
        FamilyRuleAction::RemoveMemberMissingTarget => fx
            .family
            .remove_member_at(fx.owner_user_id, target, now)
            .map(|_| ()),
        _ => unreachable!("non-governance action"),
    })
}

fn execute_non_owner_profile(fx: &mut FamilyFixture) -> Result<Result<(), DomainError>, DomainError> {
    let name = FamilyName::try_new("Renamed")?;
    let currency = CurrencyCode::try_new("EUR")?;
    Ok(fx.family.update_profile(fx.member_user_id, name, currency))
}

fn accept_special_invite(
    given: FamilyGivenState,
    fx: &mut FamilyFixture,
    bob: MemberDisplayName,
    now: DateTime<Utc>,
) -> Result<(), DomainError> {
    let (token_id, accept_at) = match (given, fx.invite.as_ref()) {
        (FamilyGivenState::ExpiredInviteAtBoundary, Some(inv)) => (inv.token(), inv.expires_at()),
        (FamilyGivenState::ExpiredInviteAfterBoundary, Some(inv)) => {
            (inv.token(), inv.expires_at() + Duration::seconds(1))
        }
        (FamilyGivenState::UsedInvite, Some(inv)) => (inv.token(), now),
        _ => (InviteTokenId::new_v4(), now),
    };
    fx.family
        .accept_invite_at(token_id, UserId::new_v4(), bob, accept_at)
        .map(|_| ())
}
