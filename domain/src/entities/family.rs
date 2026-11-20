//! Family aggregate root — manages members, roles, permissions, and invite tokens.
//!
//! # Invariants enforced:
//! - A family must always have exactly one `Owner`.
//! - The `Owner` role cannot be changed or removed by anyone (including the owner).
//! - No second member can be invited or promoted to `Owner`.
//! - Only the `Owner` can invite new members, change roles, or remove members.
//! - Custom [`Permission`] flags can only be configured for [`Role::Other`].
//! - Member display names cannot be empty or whitespace-only (enforced by [`DisplayName`]).
//! - Family names cannot be empty or whitespace-only (enforced by [`FamilyName`]).
//! - Invite tokens expire after the configured duration (must be positive).
//! - Invite tokens cannot be used more than once.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    errors::DomainError,
    events::DomainEvent,
    value_objects::{
        CurrencyCode, DisplayName, FamilyId, FamilyName, InviteTokenId, InviteTtl, MemberId,
        Permission, Relationship, Role, UserId,
    },
};

/// The `Family` aggregate root.
///
/// Encapsulates the family profile, its member roster, and pending invite tokens.
/// All mutations emit domain events and enforce family-level invariants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Family {
    /// UUID v4 — random PK, avoids write hotspots on Aurora DSQL.
    pub id: FamilyId,
    /// Validated, trimmed family name (1–200 chars).
    pub name: FamilyName,
    /// ISO 4217 currency code for the family's default display currency.
    pub home_currency: CurrencyCode,
    /// All members of this family, including soft-deleted ones.
    pub members: Vec<FamilyMember>,
    /// All invite tokens issued for this family.
    pub invite_tokens: Vec<InviteToken>,
    pub created_at: DateTime<Utc>,
    /// Soft-delete timestamp. `None` means the family is active.
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A member of a family. Child entity of `Family`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyMember {
    /// UUID v4 — random PK, avoids write hotspots on Aurora DSQL.
    pub id: MemberId,
    pub family_id: FamilyId,
    /// Cognito `sub` — the user's permanent identifier from JWT claims.
    pub user_id: UserId,
    /// Validated, trimmed display name (1–100 chars). Enforced by [`DisplayName`].
    pub display_name: DisplayName,
    pub role: Role,
    /// Custom capability flags (only used when `role == Role::Other`).
    pub permissions: Vec<Permission>,
    /// Optional family relationship label for any member (e.g., "Spouse", "Daughter", "Grandma", "Cousin").
    pub relationship: Option<Relationship>,
    pub joined_at: DateTime<Utc>,
    /// Soft-delete — never hard-delete members.
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A one-time invite token that allows a new user to join the family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteToken {
    /// UUID v4 — used as PK in database (`family_invite_tokens.token`) and URL token.
    pub token: InviteTokenId,
    pub family_id: FamilyId,
    pub role: Role,
    /// Custom capability flags (only used when `role == Role::Other`).
    pub permissions: Vec<Permission>,
    /// Optional family relationship label (e.g., "Spouse", "Daughter", "Grandma", "Cousin").
    pub relationship: Option<Relationship>,
    /// References `family_members.id` of the member who created the invite.
    pub created_by: MemberId,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
}

impl Family {
    /// Creates a new `Family` aggregate with the creator as the sole `Owner`.
    pub fn create(
        name: FamilyName,
        home_currency: CurrencyCode,
        owner_user_id: UserId,
        owner_display_name: DisplayName,
    ) -> Result<(Self, Vec<DomainEvent>), DomainError> {
        let family_id = FamilyId::new_v4();
        let owner_member_id = MemberId::new_v4();
        let now = Utc::now();
        let owner = build_owner_member(
            owner_member_id,
            family_id,
            owner_user_id,
            owner_display_name,
            now,
        );
        let events = build_creation_events(
            family_id,
            owner_member_id,
            owner_user_id,
            &name,
            &home_currency,
            now,
        );
        let family = Self {
            id: family_id,
            name,
            home_currency,
            members: vec![owner],
            invite_tokens: vec![],
            created_at: now,
            deleted_at: None,
        };
        Ok((family, events))
    }

    /// Creates a new invite token for a prospective member.
    ///
    /// Only `Owner` can invite members, `role` cannot be `Owner`, custom
    /// `permissions` are allowed only for `Role::Other`, `relationship` is an
    /// optional label allowed for any role, and [`InviteTtl`] guarantees a
    /// positive expiration duration.
    pub fn create_invite(
        &mut self,
        created_by_user_id: UserId,
        role: Role,
        permissions: Vec<Permission>,
        relationship: Option<Relationship>,
        ttl: InviteTtl,
    ) -> Result<(InviteToken, Vec<DomainEvent>), DomainError> {
        let creator_member_id = self.authorize_owner(created_by_user_id)?;
        validate_invite_params(role, &permissions)?;

        let now = Utc::now();
        let token = InviteTokenId::new_v4();
        let invite = InviteToken {
            token,
            family_id: self.id,
            role,
            permissions: permissions.clone(),
            relationship,
            created_by: creator_member_id,
            created_at: now,
            expires_at: now + ttl.as_duration(),
            used: false,
        };
        self.invite_tokens.push(invite.clone());

        let events = vec![DomainEvent::MemberInvited {
            family_id: self.id,
            invite_token: token,
            role,
            permissions,
            created_by: creator_member_id,
            occurred_at: now,
        }];
        Ok((invite, events))
    }

    /// Accepts an invite token and adds the user as a new member.
    pub fn accept_invite(
        &mut self,
        token: InviteTokenId,
        user_id: UserId,
        display_name: DisplayName,
    ) -> Result<(FamilyMember, Vec<DomainEvent>), DomainError> {
        let now = Utc::now();
        let (role, permissions, relationship) = self.consume_invite(token, now)?;
        let member_id = MemberId::new_v4();
        let member = FamilyMember {
            id: member_id,
            family_id: self.id,
            user_id,
            display_name,
            role,
            permissions,
            relationship,
            joined_at: now,
            deleted_at: None,
        };
        self.members.push(member.clone());

        let events = vec![DomainEvent::MemberJoined {
            family_id: self.id,
            member_id,
            user_id,
            role,
            occurred_at: now,
        }];
        Ok((member, events))
    }

    /// Changes a member's role.
    ///
    /// Only `Owner` can change roles; the Owner cannot be demoted and no second
    /// member can be promoted to `Owner`.
    pub fn change_member_role(
        &mut self,
        requester_user_id: UserId,
        target_member_id: MemberId,
        new_role: Role,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        let requester_member_id = self.authorize_owner(requester_user_id)?;
        let now = Utc::now();
        let target = self.find_active_member_by_id_mut(target_member_id)?;

        if target.role == Role::Owner && new_role != Role::Owner {
            return Err(DomainError::CannotRemoveOwner);
        }
        if new_role == Role::Owner && target.role != Role::Owner {
            return Err(DomainError::DuplicateOwner);
        }

        let old_role = target.role;
        target.role = new_role;
        if new_role != Role::Other {
            target.permissions.clear();
        }

        Ok(vec![DomainEvent::MemberRoleChanged {
            family_id: self.id,
            member_id: target_member_id,
            old_role,
            new_role,
            changed_by: requester_member_id,
            occurred_at: now,
        }])
    }

    /// Soft-deletes a member (sets `deleted_at`).
    pub fn remove_member(
        &mut self,
        requester_user_id: UserId,
        target_member_id: MemberId,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        let requester_member_id = self.authorize_owner(requester_user_id)?;
        let now = Utc::now();
        let target = self.find_active_member_by_id_mut(target_member_id)?;

        if target.role == Role::Owner {
            return Err(DomainError::CannotRemoveOwner);
        }
        target.deleted_at = Some(now);

        Ok(vec![DomainEvent::MemberRemoved {
            family_id: self.id,
            member_id: target_member_id,
            removed_by: requester_member_id,
            occurred_at: now,
        }])
    }

    fn authorize_owner(&self, user_id: UserId) -> Result<MemberId, DomainError> {
        let member = self
            .members
            .iter()
            .find(|m| m.user_id == user_id && m.deleted_at.is_none())
            .ok_or(DomainError::UserNotInFamily { user_id })?;
        member.role.assert_family_management()?;
        Ok(member.id)
    }

    fn consume_invite(
        &mut self,
        token: InviteTokenId,
        now: DateTime<Utc>,
    ) -> Result<(Role, Vec<Permission>, Option<Relationship>), DomainError> {
        let invite = self
            .invite_tokens
            .iter_mut()
            .find(|t| t.token == token)
            .ok_or(DomainError::InvariantViolation {
                message: format!("Invite token not found: {}", token),
            })?;

        if invite.used {
            return Err(DomainError::TokenAlreadyUsed);
        }
        if invite.expires_at < now {
            return Err(DomainError::TokenExpired);
        }

        invite.used = true;
        Ok((
            invite.role,
            invite.permissions.clone(),
            invite.relationship.clone(),
        ))
    }

    fn find_active_member_by_id_mut(
        &mut self,
        target_member_id: MemberId,
    ) -> Result<&mut FamilyMember, DomainError> {
        self.members
            .iter_mut()
            .find(|m| m.id == target_member_id && m.deleted_at.is_none())
            .ok_or(DomainError::MemberNotFound {
                member_id: target_member_id,
            })
    }
}

impl FamilyMember {
    /// Returns `true` if this member is currently active (not soft-deleted).
    pub fn is_active(&self) -> bool {
        self.deleted_at.is_none()
    }

    /// Evaluates whether this member has `perm`, respecting role rules and custom permissions.
    pub fn has_permission(&self, perm: Permission) -> bool {
        self.role.has_permission(&self.permissions, perm)
    }
}

fn build_owner_member(
    id: MemberId,
    family_id: FamilyId,
    user_id: UserId,
    display_name: DisplayName,
    joined_at: DateTime<Utc>,
) -> FamilyMember {
    FamilyMember {
        id,
        family_id,
        user_id,
        display_name,
        role: Role::Owner,
        permissions: Vec::new(),
        relationship: None,
        joined_at,
        deleted_at: None,
    }
}

fn build_creation_events(
    family_id: FamilyId,
    owner_member_id: MemberId,
    owner_user_id: UserId,
    name: &FamilyName,
    home_currency: &CurrencyCode,
    occurred_at: DateTime<Utc>,
) -> Vec<DomainEvent> {
    vec![
        DomainEvent::FamilyCreated {
            family_id,
            name: name.clone(),
            home_currency: home_currency.clone(),
            occurred_at,
        },
        DomainEvent::MemberJoined {
            family_id,
            member_id: owner_member_id,
            user_id: owner_user_id,
            role: Role::Owner,
            occurred_at,
        },
    ]
}

fn validate_invite_params(role: Role, permissions: &[Permission]) -> Result<(), DomainError> {
    if role == Role::Owner {
        return Err(DomainError::DuplicateOwner);
    }
    if role != Role::Other && !permissions.is_empty() {
        return Err(DomainError::InvariantViolation {
            message: "Custom permissions can only be configured for Role::Other".to_string(),
        });
    }
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn make_family() -> (Family, UserId) {
        let owner_id = UserId::new_v4();
        let currency = CurrencyCode::new("USD").unwrap();
        let name = FamilyName::try_new("Test Family").unwrap();
        let owner_name = DisplayName::try_new("Alice").unwrap();
        let (family, _) = Family::create(name, currency, owner_id, owner_name).unwrap();
        (family, owner_id)
    }

    #[test]
    fn family_create_succeeds() {
        let (family, _) = make_family();
        assert_eq!(family.name.as_ref(), "Test Family");
        assert_eq!(family.members.len(), 1);
        assert_eq!(family.members[0].role, Role::Owner);
    }

    #[test]
    fn family_name_empty_is_rejected_by_value_object() {
        assert!(FamilyName::try_new("").is_err());
        assert!(FamilyName::try_new("  ").is_err());
    }

    #[test]
    fn display_name_empty_is_rejected_by_value_object() {
        assert!(DisplayName::try_new("").is_err());
        assert!(DisplayName::try_new("  ").is_err());
    }

    #[test]
    fn non_owner_cannot_invite() {
        let (mut family, owner_id) = make_family();
        let (invite, _) = family
            .create_invite(
                owner_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(48).unwrap(),
            )
            .unwrap();
        let member_user_id = UserId::new_v4();
        let member_name = DisplayName::try_new("Bob").unwrap();
        family
            .accept_invite(invite.token, member_user_id, member_name)
            .unwrap();

        let result = family.create_invite(
            member_user_id,
            Role::Child,
            Vec::new(),
            None,
            InviteTtl::from_hours(24).unwrap(),
        );
        assert!(matches!(
            result,
            Err(DomainError::InsufficientRole { role: Role::Member })
        ));
    }

    #[test]
    fn invite_for_owner_role_is_rejected() {
        let (mut family, owner_id) = make_family();
        let result = family.create_invite(
            owner_id,
            Role::Owner,
            Vec::new(),
            None,
            InviteTtl::from_hours(24).unwrap(),
        );
        assert!(matches!(result, Err(DomainError::DuplicateOwner)));
    }

    #[test]
    fn custom_permissions_rejected_for_non_other_roles() {
        let (mut family, owner_id) = make_family();
        let result = family.create_invite(
            owner_id,
            Role::Member,
            vec![Permission::ViewLoans],
            None,
            InviteTtl::from_hours(24).unwrap(),
        );
        assert!(matches!(
            result,
            Err(DomainError::InvariantViolation { .. })
        ));
    }

    #[test]
    fn other_role_invite_propagates_permissions_and_creator_member_id() {
        let (mut family, owner_id) = make_family();
        let owner_member_id = family.members[0].id;
        let perms = vec![Permission::ViewBudget, Permission::RecordOwnTransactions];
        let relationship = Relationship::try_new("Grandma").unwrap();

        let (invite, _) = family
            .create_invite(
                owner_id,
                Role::Other,
                perms.clone(),
                Some(relationship.clone()),
                InviteTtl::from_hours(48).unwrap(),
            )
            .unwrap();
        assert_eq!(invite.created_by, owner_member_id);
        assert_eq!(invite.permissions, perms);
        assert_eq!(invite.relationship.as_ref(), Some(&relationship));

        let (member, _) = family
            .accept_invite(
                invite.token,
                UserId::new_v4(),
                DisplayName::try_new("Elena").unwrap(),
            )
            .unwrap();
        assert_eq!(member.permissions, perms);
        assert_eq!(member.relationship, Some(relationship));
        assert!(member.has_permission(Permission::ViewBudget));
        assert!(!member.has_permission(Permission::ViewLoans));
    }

    #[test]
    fn relationship_is_allowed_on_non_other_roles() {
        let (mut family, owner_id) = make_family();
        let relationship = Relationship::try_new("Spouse").unwrap();

        let (invite, _) = family
            .create_invite(
                owner_id,
                Role::Member,
                Vec::new(),
                Some(relationship.clone()),
                InviteTtl::from_hours(48).unwrap(),
            )
            .unwrap();
        let (member, _) = family
            .accept_invite(
                invite.token,
                UserId::new_v4(),
                DisplayName::try_new("Partner").unwrap(),
            )
            .unwrap();

        assert_eq!(member.role, Role::Member);
        assert_eq!(member.relationship, Some(relationship));
    }

    #[test]
    fn invite_ttl_zero_or_negative_is_rejected_by_value_object() {
        assert!(InviteTtl::try_from_duration(chrono::Duration::zero()).is_err());
        assert!(InviteTtl::try_from_duration(chrono::Duration::hours(-1)).is_err());
        assert!(InviteTtl::from_hours(0).is_err());
        assert!(InviteTtl::from_hours(-1).is_err());
    }

    #[test]
    fn used_token_cannot_be_reused() {
        let (mut family, owner_id) = make_family();
        let (invite, _) = family
            .create_invite(
                owner_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(48).unwrap(),
            )
            .unwrap();

        let _ = family.accept_invite(
            invite.token,
            UserId::new_v4(),
            DisplayName::try_new("Bob").unwrap(),
        );
        let result = family.accept_invite(
            invite.token,
            UserId::new_v4(),
            DisplayName::try_new("Carol").unwrap(),
        );

        assert!(matches!(result, Err(DomainError::TokenAlreadyUsed)));
    }

    #[test]
    fn owner_cannot_be_removed() {
        let (mut family, owner_id) = make_family();
        let owner_member_id = family.members[0].id;

        let result = family.remove_member(owner_id, owner_member_id);
        assert!(matches!(result, Err(DomainError::CannotRemoveOwner)));
    }

    #[test]
    fn non_member_caller_returns_user_not_in_family_and_missing_target_returns_member_not_found() {
        let (mut family, owner_id) = make_family();
        let outsider_user_id = UserId::new_v4();
        let unknown_member_id = MemberId::new_v4();

        let invite_err = family
            .create_invite(
                outsider_user_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(24).unwrap(),
            )
            .unwrap_err();
        assert_eq!(
            invite_err,
            DomainError::UserNotInFamily {
                user_id: outsider_user_id
            }
        );

        let remove_outsider_err = family
            .remove_member(outsider_user_id, unknown_member_id)
            .unwrap_err();
        assert_eq!(
            remove_outsider_err,
            DomainError::UserNotInFamily {
                user_id: outsider_user_id
            }
        );

        let missing_target_err = family
            .remove_member(owner_id, unknown_member_id)
            .unwrap_err();
        assert_eq!(
            missing_target_err,
            DomainError::MemberNotFound {
                member_id: unknown_member_id
            }
        );
    }

    #[test]
    fn owner_cannot_change_own_role_away_from_owner() {
        let (mut family, owner_id) = make_family();
        let owner_member_id = family.members[0].id;

        let result = family.change_member_role(owner_id, owner_member_id, Role::Member);
        assert!(matches!(result, Err(DomainError::CannotRemoveOwner)));
    }

    #[test]
    fn promoting_another_member_to_owner_is_rejected() {
        let (mut family, owner_id) = make_family();
        let (invite, _) = family
            .create_invite(
                owner_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(48).unwrap(),
            )
            .unwrap();
        let (member, _) = family
            .accept_invite(
                invite.token,
                UserId::new_v4(),
                DisplayName::try_new("Bob").unwrap(),
            )
            .unwrap();

        let result = family.change_member_role(owner_id, member.id, Role::Owner);
        assert!(matches!(result, Err(DomainError::DuplicateOwner)));
    }

    #[test]
    fn member_soft_delete_sets_deleted_at() {
        let (mut family, owner_id) = make_family();
        let (invite, _) = family
            .create_invite(
                owner_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(48).unwrap(),
            )
            .unwrap();
        let (member, _) = family
            .accept_invite(
                invite.token,
                UserId::new_v4(),
                DisplayName::try_new("Bob").unwrap(),
            )
            .unwrap();

        family.remove_member(owner_id, member.id).unwrap();

        let removed = family.members.iter().find(|m| m.id == member.id).unwrap();
        assert!(removed.deleted_at.is_some());
        assert!(!removed.is_active());
    }

    #[test]
    fn family_create_emits_two_events() {
        let owner_id = UserId::new_v4();
        let currency = CurrencyCode::new("MDL").unwrap();
        let name = FamilyName::try_new("My Family").unwrap();
        let owner_name = DisplayName::try_new("Ana").unwrap();
        let (_, events) =
            Family::create(name.clone(), currency.clone(), owner_id, owner_name).unwrap();

        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            DomainEvent::FamilyCreated {
                name: event_name,
                home_currency: event_currency,
                ..
            } if event_name == &name && event_currency == &currency
        ));
        assert!(matches!(events[1], DomainEvent::MemberJoined { .. }));
    }
}
