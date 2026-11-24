//! Blind conformance tests for `backend/specs/domain/family-rbac.md` (`FM-01`..`FM-22`).

use chrono::{DateTime, Duration, Utc};

use domain::entities::{Family, FamilyParams};
use domain::errors::DomainError;
use domain::events::DomainEvent;
use domain::value_objects::{
    CurrencyCode, DisplayName, FamilyName, InviteTtl, MemberId, Permission, Relationship, Role,
    UserId,
};

fn base_time() -> DateTime<Utc> {
    DateTime::UNIX_EPOCH + Duration::days(20_000)
}

fn create_sample_family() -> Result<(Family, UserId, MemberId), DomainError> {
    let owner_user_id = UserId::new_v4();
    let name = FamilyName::try_new("Test Family")?;
    let currency = CurrencyCode::try_new("USD")?;
    let owner_name = DisplayName::try_new("Alice")?;
    let (family, _) = Family::create_at(name, currency, owner_user_id, owner_name, base_time())?;
    let owner_member_id =
        family
            .members()
            .first()
            .map(|m| m.id())
            .ok_or(DomainError::InvariantViolation {
                message: "created family must have an owner".to_owned(),
            })?;
    Ok((family, owner_user_id, owner_member_id))
}

fn invite_and_join(
    family: &mut Family,
    owner_user_id: UserId,
    role: Role,
    permissions: Vec<Permission>,
    relationship: Option<Relationship>,
    join_user_id: UserId,
) -> Result<MemberId, DomainError> {
    let ttl = InviteTtl::from_days(7)?;
    let (token, _) = family.create_invite_at(
        owner_user_id,
        role,
        permissions,
        relationship,
        ttl,
        base_time(),
    )?;
    let display = DisplayName::try_new("Bob")?;
    let (member, _) = family.accept_invite_at(
        token.token(),
        join_user_id,
        display,
        base_time() + Duration::hours(1),
    )?;
    Ok(member.id())
}

#[test]
fn fm_01_owner_has_all_eight_permissions() {
    for perm in Permission::all() {
        assert!(Role::Owner.has_permission(&[], perm));
    }
}

#[test]
fn fm_02_member_has_all_eight_permissions_but_cannot_manage_family() {
    for perm in Permission::all() {
        assert!(Role::Member.has_permission(&[], perm));
    }
    assert!(!Role::Member.can_manage_family());
}

#[test]
fn fm_03_child_ignores_custom_view_loans_permission() {
    assert!(!Role::Child.has_permission(&[Permission::ViewLoans], Permission::ViewLoans));
}

#[test]
fn fm_04_child_can_record_own_transactions() {
    assert!(Role::Child.has_permission(&[], Permission::RecordOwnTransactions));
}

#[test]
fn fm_05_other_with_custom_view_loans_has_permission() {
    assert!(Role::Other.has_permission(&[Permission::ViewLoans], Permission::ViewLoans));
}

#[test]
fn fm_06_child_debt_access_is_denied_even_with_custom_flag() {
    let err = Role::Child
        .assert_debt_access(&[Permission::ViewLoans])
        .unwrap_err();
    assert_eq!(err, DomainError::InsufficientRole { role: Role::Child });
}

#[test]
fn fm_07_other_with_view_loans_passes_debt_access() {
    assert!(
        Role::Other
            .assert_debt_access(&[Permission::ViewLoans])
            .is_ok()
    );
}

#[test]
fn fm_08_owner_passes_family_management() {
    assert!(Role::Owner.assert_family_management().is_ok());
}

#[test]
fn fm_09_other_without_view_loans_fails_debt_access() {
    let err = Role::Other.assert_debt_access(&[]).unwrap_err();
    assert_eq!(err, DomainError::InsufficientRole { role: Role::Other });
}

#[test]
fn fm_10_other_denies_absent_custom_permission() {
    assert!(!Role::Other.has_permission(&[Permission::ViewLoans], Permission::ViewBudget));
}

#[test]
fn fm_11_owner_passes_debt_access() {
    assert!(Role::Owner.assert_debt_access(&[]).is_ok());
}

#[test]
fn fm_12_member_passes_debt_access() {
    assert!(Role::Member.assert_debt_access(&[]).is_ok());
}

#[test]
fn fm_13_non_owner_roles_cannot_manage_family_even_with_all_flags() {
    for role in [Role::Member, Role::Child, Role::Other] {
        assert!(
            !role.has_permission(&Permission::all(), Permission::ViewBudget) || role != Role::Child
        );
        let err = role.assert_family_management().unwrap_err();
        assert_eq!(err, DomainError::InsufficientRole { role });
    }
}

#[test]
fn fm_14_child_can_manage_own_accounts() {
    assert!(Role::Child.has_permission(&[], Permission::ManageOwnAccounts));
}

#[test]
fn fm_15_accept_invite_rejects_already_active_member_without_consuming_token() {
    let (mut family, owner_user_id, _) = create_sample_family().unwrap();
    let existing_user = UserId::new_v4();
    invite_and_join(
        &mut family,
        owner_user_id,
        Role::Member,
        vec![],
        None,
        existing_user,
    )
    .unwrap();

    let (second_token, _) = family
        .create_invite_at(
            owner_user_id,
            Role::Member,
            vec![],
            None,
            InviteTtl::from_days(7).unwrap(),
            base_time() + Duration::hours(2),
        )
        .unwrap();

    let err = family
        .accept_invite_at(
            second_token.token(),
            existing_user,
            DisplayName::try_new("Bob Again").unwrap(),
            base_time() + Duration::hours(3),
        )
        .unwrap_err();
    assert_eq!(
        err,
        DomainError::MemberAlreadyExists {
            user_id: existing_user
        }
    );

    let stored = family
        .invite_tokens()
        .iter()
        .find(|t| t.token() == second_token.token())
        .unwrap();
    assert!(!stored.is_used());
}

#[test]
fn fm_16_accept_invite_reactivates_soft_deleted_member_preserving_id() {
    let (mut family, owner_user_id, _) = create_sample_family().unwrap();
    let member_user = UserId::new_v4();
    let original_member_id = invite_and_join(
        &mut family,
        owner_user_id,
        Role::Member,
        vec![],
        None,
        member_user,
    )
    .unwrap();

    family
        .remove_member_at(
            owner_user_id,
            original_member_id,
            base_time() + Duration::hours(2),
        )
        .unwrap();

    let rel = Relationship::try_new("Grandma").unwrap();
    let (reinvite, _) = family
        .create_invite_at(
            owner_user_id,
            Role::Other,
            vec![Permission::ViewBudget],
            Some(rel.clone()),
            InviteTtl::from_days(7).unwrap(),
            base_time() + Duration::hours(3),
        )
        .unwrap();

    let (reactivated, events) = family
        .accept_invite_at(
            reinvite.token(),
            member_user,
            DisplayName::try_new("Grandma Alice").unwrap(),
            base_time() + Duration::hours(4),
        )
        .unwrap();

    assert_eq!(reactivated.id(), original_member_id);
    assert!(reactivated.deleted_at().is_none());
    assert!(reactivated.is_active());
    assert_eq!(reactivated.display_name().as_str(), "Grandma Alice");
    assert_eq!(reactivated.role(), Role::Other);
    assert_eq!(reactivated.permissions(), &[Permission::ViewBudget]);
    assert_eq!(reactivated.relationship(), Some(&rel));
    assert!(matches!(
        events.as_slice(),
        [DomainEvent::MemberJoined { member_id, user_id, role: Role::Other, .. }]
            if *member_id == original_member_id && *user_id == member_user
    ));
}

#[test]
fn fm_17_soft_deleted_family_rejects_all_mutations() {
    let (active_family, owner_user_id, owner_member_id) = create_sample_family().unwrap();
    let mut deleted = Family::reconstitute(FamilyParams {
        id: active_family.id(),
        name: active_family.name().clone(),
        home_currency: active_family.home_currency().clone(),
        members: active_family.members().to_vec(),
        invite_tokens: active_family.invite_tokens().to_vec(),
        created_at: active_family.created_at(),
        deleted_at: Some(base_time() + Duration::hours(1)),
    });
    assert!(deleted.is_deleted());

    let now = base_time() + Duration::hours(2);
    assert_eq!(
        deleted
            .create_invite_at(
                owner_user_id,
                Role::Member,
                vec![],
                None,
                InviteTtl::from_days(1).unwrap(),
                now
            )
            .unwrap_err(),
        DomainError::FamilyDeleted
    );
    assert_eq!(
        deleted
            .accept_invite_at(
                domain::value_objects::InviteTokenId::new_v4(),
                UserId::new_v4(),
                DisplayName::try_new("Bob").unwrap(),
                now
            )
            .unwrap_err(),
        DomainError::FamilyDeleted
    );
    assert_eq!(
        deleted
            .change_member_role_at(owner_user_id, owner_member_id, Role::Member, vec![], now)
            .unwrap_err(),
        DomainError::FamilyDeleted
    );
    assert_eq!(
        deleted
            .remove_member_at(owner_user_id, owner_member_id, now)
            .unwrap_err(),
        DomainError::FamilyDeleted
    );
}

#[test]
fn fm_18_expired_invite_token_is_rejected_at_or_after_expiration() {
    let (mut family, owner_user_id, _) = create_sample_family().unwrap();
    let ttl = InviteTtl::try_new(60).unwrap();
    let (invite, _) = family
        .create_invite_at(owner_user_id, Role::Member, vec![], None, ttl, base_time())
        .unwrap();

    let at_expiry = family
        .accept_invite_at(
            invite.token(),
            UserId::new_v4(),
            DisplayName::try_new("Bob").unwrap(),
            invite.expires_at(),
        )
        .unwrap_err();
    assert_eq!(at_expiry, DomainError::TokenExpired);

    let after_expiry = family
        .accept_invite_at(
            invite.token(),
            UserId::new_v4(),
            DisplayName::try_new("Bob").unwrap(),
            invite.expires_at() + Duration::seconds(1),
        )
        .unwrap_err();
    assert_eq!(after_expiry, DomainError::TokenExpired);
}

#[test]
fn fm_19_unknown_invite_token_returns_invite_token_not_found() {
    let (mut family, _, _) = create_sample_family().unwrap();
    let err = family
        .accept_invite_at(
            domain::value_objects::InviteTokenId::new_v4(),
            UserId::new_v4(),
            DisplayName::try_new("Bob").unwrap(),
            base_time(),
        )
        .unwrap_err();
    assert_eq!(err, DomainError::InviteTokenNotFound);
}

#[test]
fn fm_20_change_member_role_deduplicates_other_permissions_and_emits_event() {
    let (mut family, owner_user_id, _) = create_sample_family().unwrap();
    let target_member_id = invite_and_join(
        &mut family,
        owner_user_id,
        Role::Member,
        vec![],
        None,
        UserId::new_v4(),
    )
    .unwrap();

    let events = family
        .change_member_role_at(
            owner_user_id,
            target_member_id,
            Role::Other,
            vec![Permission::ViewBudget, Permission::ViewBudget],
            base_time() + Duration::hours(2),
        )
        .unwrap();

    let updated = family
        .members()
        .iter()
        .find(|m| m.id() == target_member_id)
        .unwrap();
    assert_eq!(updated.role(), Role::Other);
    assert_eq!(updated.permissions(), &[Permission::ViewBudget]);
    assert!(matches!(
        events.as_slice(),
        [DomainEvent::MemberRoleChanged {
            member_id,
            old_role: Role::Member,
            new_role: Role::Other,
            permissions,
            ..
        }] if *member_id == target_member_id && permissions == &[Permission::ViewBudget]
    ));
}

#[test]
fn fm_21_change_member_role_noop_emits_zero_events() {
    let (mut family, owner_user_id, _) = create_sample_family().unwrap();
    let target_member_id = invite_and_join(
        &mut family,
        owner_user_id,
        Role::Other,
        vec![Permission::ViewBudget],
        None,
        UserId::new_v4(),
    )
    .unwrap();

    let events = family
        .change_member_role_at(
            owner_user_id,
            target_member_id,
            Role::Other,
            vec![Permission::ViewBudget],
            base_time() + Duration::hours(2),
        )
        .unwrap();
    assert!(events.is_empty());
}

#[test]
fn fm_22_owner_cannot_demote_or_remove_self_nor_promote_another_owner() {
    let (mut family, owner_user_id, owner_member_id) = create_sample_family().unwrap();
    let member_id = invite_and_join(
        &mut family,
        owner_user_id,
        Role::Member,
        vec![],
        None,
        UserId::new_v4(),
    )
    .unwrap();
    let now = base_time() + Duration::hours(2);

    assert_eq!(
        family
            .change_member_role_at(owner_user_id, owner_member_id, Role::Member, vec![], now)
            .unwrap_err(),
        DomainError::CannotRemoveOwner
    );
    assert_eq!(
        family
            .remove_member_at(owner_user_id, owner_member_id, now)
            .unwrap_err(),
        DomainError::CannotRemoveOwner
    );
    assert_eq!(
        family
            .change_member_role_at(owner_user_id, member_id, Role::Owner, vec![], now)
            .unwrap_err(),
        DomainError::DuplicateOwner
    );
}
