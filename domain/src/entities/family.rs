//! `Family` aggregate root — Identity & Family bounded context.
//!
//! The `Family` is the central aggregate root. All other entities belong to
//! a family and are accessed through a `family_id` qualifier in every query.
//!
//! # Invariants enforced by this aggregate
//!
//! 1. A family must always have exactly one Owner.
//! 2. The Owner cannot be removed (use `change_role` first to demote them).
//! 3. `family_id` is never trusted from client input — it comes from the JWT.
//! 4. Soft-deletes only — never hard-delete members.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    errors::DomainError,
    events::DomainEvent,
    value_objects::{CurrencyCode, DisplayName, FamilyName, Role},
};

// ─── Family Aggregate Root ────────────────────────────────────────────────────

/// The Family aggregate root.
///
/// Owns a list of members and invite tokens.
/// All mutations return a `Vec<DomainEvent>` for publication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Family {
    pub id: Uuid,
    /// Validated, trimmed family name (1–100 chars). Enforced by [`FamilyName`].
    pub name: FamilyName,
    pub home_currency: CurrencyCode,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,

    pub members: Vec<FamilyMember>,
    pub invite_tokens: Vec<InviteToken>,
}

impl Family {
    /// Creates a new family with the founding owner.
    ///
    /// Both `name` and `owner_display_name` are validated value objects —
    /// construction is rejected at the call site before reaching this method.
    ///
    /// Returns the new `Family` and the `FamilyCreated` + `MemberJoined` events.
    pub fn create(
        name: FamilyName,
        home_currency: CurrencyCode,
        owner_user_id: Uuid,
        owner_display_name: DisplayName,
    ) -> Result<(Self, Vec<DomainEvent>), DomainError> {
        let now = Utc::now();
        let family_id = Uuid::new_v4();
        let owner_member_id = Uuid::new_v4();

        let owner = FamilyMember {
            id: owner_member_id,
            family_id,
            user_id: owner_user_id,
            display_name: owner_display_name,
            role: Role::Owner,
            relationship: None,
            joined_at: now,
            deleted_at: None,
        };

        let family = Self {
            id: family_id,
            name: name.clone(),
            home_currency: home_currency.clone(),
            created_at: now,
            deleted_at: None,
            members: vec![owner],
            invite_tokens: vec![],
        };

        let events = vec![
            DomainEvent::FamilyCreated {
                family_id,
                name: name.into_inner(),
                home_currency: home_currency.to_string(),
                occurred_at: now,
            },
            DomainEvent::MemberJoined {
                family_id,
                member_id: owner_member_id,
                user_id: owner_user_id,
                role: Role::Owner,
                occurred_at: now,
            },
        ];

        Ok((family, events))
    }

    /// Creates a new invite token for a prospective member.
    ///
    /// # Authorization
    /// Only `Owner` can invite members.
    pub fn create_invite(
        &mut self,
        created_by_user_id: Uuid,
        role: Role,
        relationship: Option<String>,
        expires_in_hours: u32,
    ) -> Result<(InviteToken, Vec<DomainEvent>), DomainError> {
        // Only owner can invite
        let creator = self.find_active_member_by_user_id(created_by_user_id)?;
        creator.role.assert_family_management()?;

        let now = Utc::now();
        let token_str = Uuid::new_v4().to_string();
        let expires_at = now + chrono::Duration::hours(expires_in_hours as i64);

        let invite = InviteToken {
            token: token_str.clone(),
            family_id: self.id,
            role,
            relationship: relationship.clone(),
            created_by: created_by_user_id,
            expires_at,
            used: false,
        };

        self.invite_tokens.push(invite.clone());

        let events = vec![DomainEvent::MemberInvited {
            family_id: self.id,
            invite_token: token_str,
            role,
            created_by: created_by_user_id,
            occurred_at: now,
        }];

        Ok((invite, events))
    }

    /// Accepts an invite token and adds the user as a new member.
    ///
    /// `display_name` is a validated value object — empty or whitespace-only
    /// names are rejected by [`DisplayName`] before this call is reached.
    pub fn accept_invite(
        &mut self,
        token_str: &str,
        user_id: Uuid,
        display_name: DisplayName,
    ) -> Result<(FamilyMember, Vec<DomainEvent>), DomainError> {
        let now = Utc::now();

        let invite = self
            .invite_tokens
            .iter_mut()
            .find(|t| t.token == token_str)
            .ok_or(DomainError::InvariantViolation {
                message: format!("Invite token not found: {}", token_str),
            })?;

        if invite.used {
            return Err(DomainError::TokenAlreadyUsed);
        }
        if invite.expires_at < now {
            return Err(DomainError::TokenExpired);
        }

        invite.used = true;

        let member_id = Uuid::new_v4();
        let role = invite.role;
        let relationship = invite.relationship.clone();

        let member = FamilyMember {
            id: member_id,
            family_id: self.id,
            user_id,
            display_name,
            role,
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
    /// # Authorization
    /// Only Owner can change roles.
    /// Cannot change the Owner's own role (would leave family without an owner).
    pub fn change_member_role(
        &mut self,
        requester_user_id: Uuid,
        target_member_id: Uuid,
        new_role: Role,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        let requester = self.find_active_member_by_user_id(requester_user_id)?;
        requester.role.assert_family_management()?;

        let now = Utc::now();

        let target = self
            .members
            .iter_mut()
            .find(|m| m.id == target_member_id && m.deleted_at.is_none())
            .ok_or(DomainError::MemberNotFound {
                member_id: target_member_id.to_string(),
            })?;

        if target.role == Role::Owner && new_role != Role::Owner {
            return Err(DomainError::CannotRemoveOwner);
        }

        let old_role = target.role;
        target.role = new_role;

        Ok(vec![DomainEvent::MemberRoleChanged {
            family_id: self.id,
            member_id: target_member_id,
            old_role,
            new_role,
            changed_by: requester_user_id,
            occurred_at: now,
        }])
    }

    /// Soft-deletes a member (sets `deleted_at`).
    ///
    /// # Authorization
    /// Only Owner can remove members. Owner cannot remove themselves.
    pub fn remove_member(
        &mut self,
        requester_user_id: Uuid,
        target_member_id: Uuid,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        let requester = self.find_active_member_by_user_id(requester_user_id)?;
        requester.role.assert_family_management()?;

        let now = Utc::now();

        let target = self
            .members
            .iter_mut()
            .find(|m| m.id == target_member_id && m.deleted_at.is_none())
            .ok_or(DomainError::MemberNotFound {
                member_id: target_member_id.to_string(),
            })?;

        if target.role == Role::Owner {
            return Err(DomainError::CannotRemoveOwner);
        }

        target.deleted_at = Some(now);

        Ok(vec![DomainEvent::MemberRemoved {
            family_id: self.id,
            member_id: target_member_id,
            removed_by: requester_user_id,
            occurred_at: now,
        }])
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn find_active_member_by_user_id(&self, user_id: Uuid) -> Result<&FamilyMember, DomainError> {
        self.members
            .iter()
            .find(|m| m.user_id == user_id && m.deleted_at.is_none())
            .ok_or(DomainError::MemberNotFound {
                member_id: user_id.to_string(),
            })
    }
}

// ─── FamilyMember Entity ──────────────────────────────────────────────────────

/// A member of a family. Child entity of `Family`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyMember {
    /// UUID v4 — random PK, avoids write hotspots on Aurora DSQL.
    pub id: Uuid,
    pub family_id: Uuid,
    /// Cognito `sub` — the user's permanent identifier from JWT claims.
    pub user_id: Uuid,
    /// Validated, trimmed display name (1–80 chars). Enforced by [`DisplayName`].
    pub display_name: DisplayName,
    pub role: Role,
    /// Optional label for `Role::Other` members (e.g., "Grandma", "Cousin").
    pub relationship: Option<String>,
    pub joined_at: DateTime<Utc>,
    /// Soft-delete — never hard-delete members.
    pub deleted_at: Option<DateTime<Utc>>,
}

impl FamilyMember {
    /// Returns `true` if this member is currently active (not soft-deleted).
    pub fn is_active(&self) -> bool {
        self.deleted_at.is_none()
    }
}

// ─── InviteToken Entity ───────────────────────────────────────────────────────

/// A one-time invite token that allows a new user to join the family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteToken {
    /// UUID string — used as the URL token.
    pub token: String,
    pub family_id: Uuid,
    pub role: Role,
    pub relationship: Option<String>,
    pub created_by: Uuid,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn make_family() -> (Family, Uuid) {
        let owner_id = Uuid::new_v4();
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

        // First add a member via invite
        let (invite, _) = family
            .create_invite(owner_id, Role::Member, None, 48)
            .unwrap();
        let member_user_id = Uuid::new_v4();
        let member_name = DisplayName::try_new("Bob").unwrap();
        let (_, _) = family
            .accept_invite(&invite.token, member_user_id, member_name)
            .unwrap();

        // Member tries to invite — should fail
        let result = family.create_invite(member_user_id, Role::Child, None, 24);
        assert!(matches!(result, Err(DomainError::InsufficientRole { .. })));
    }

    #[test]
    fn used_token_cannot_be_reused() {
        let (mut family, owner_id) = make_family();
        let (invite, _) = family
            .create_invite(owner_id, Role::Member, None, 48)
            .unwrap();

        let _ = family.accept_invite(
            &invite.token,
            Uuid::new_v4(),
            DisplayName::try_new("Bob").unwrap(),
        );
        let result = family.accept_invite(
            &invite.token,
            Uuid::new_v4(),
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
    fn owner_cannot_change_own_role_away_from_owner() {
        let (mut family, owner_id) = make_family();
        let owner_member_id = family.members[0].id;

        let result = family.change_member_role(owner_id, owner_member_id, Role::Member);
        assert!(matches!(result, Err(DomainError::CannotRemoveOwner)));
    }

    #[test]
    fn member_soft_delete_sets_deleted_at() {
        let (mut family, owner_id) = make_family();

        let (invite, _) = family
            .create_invite(owner_id, Role::Member, None, 48)
            .unwrap();
        let member_user_id = Uuid::new_v4();
        let (member, _) = family
            .accept_invite(
                &invite.token,
                member_user_id,
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
        let owner_id = Uuid::new_v4();
        let currency = CurrencyCode::new("MDL").unwrap();
        let name = FamilyName::try_new("My Family").unwrap();
        let owner_name = DisplayName::try_new("Ana").unwrap();
        let (_, events) = Family::create(name, currency, owner_id, owner_name).unwrap();

        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], DomainEvent::FamilyCreated { .. }));
        assert!(matches!(events[1], DomainEvent::MemberJoined { .. }));
    }
}
