//! Family aggregate root — manages members, roles, permissions, and invite tokens.
//!
//! # Invariants enforced:
//! - A family must always have exactly one `Owner`.
//! - The `Owner` role cannot be changed or removed by anyone (including the owner).
//! - No second member can be invited or promoted to `Owner`.
//! - Only the `Owner` can invite new members, change roles, or remove members.
//! - Soft-deleted families cannot be mutated.
//! - Active members cannot accept a second invite into the same family; previously
//!   soft-deleted members are reactivated preserving their `MemberId`.
//! - Custom [`Permission`] flags can only be configured for [`Role::Other`].
//! - Member display names cannot be empty or whitespace-only (enforced by [`DisplayName`]).
//! - Family names cannot be empty or whitespace-only (enforced by [`FamilyName`]).
//! - Invite tokens expire after a bounded positive duration (`<= 30 days`).
//! - Invite tokens cannot be used more than once or at/after `expires_at`.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    errors::DomainError,
    events::DomainEvent,
    value_objects::{
        CurrencyCode, DisplayName, DomainAction, FamilyId, FamilyName, InviteId, InviteTokenHash,
        InviteTokenId, InviteTtl, MemberId, Permission, Relationship, Role, UserId,
    },
};

/// Parameters for reconstituting a persisted [`Family`] aggregate.
#[derive(Debug, Clone)]
pub struct FamilyParams {
    pub id: FamilyId,
    pub name: FamilyName,
    pub home_currency: CurrencyCode,
    pub members: Vec<FamilyMember>,
    pub invite_tokens: Vec<InviteToken>,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Parameters for reconstituting a persisted [`FamilyMember`] entity.
#[derive(Debug, Clone)]
pub struct FamilyMemberParams {
    pub id: MemberId,
    pub family_id: FamilyId,
    pub user_id: UserId,
    pub display_name: DisplayName,
    pub role: Role,
    pub permissions: Vec<Permission>,
    pub relationship: Option<Relationship>,
    pub joined_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Parameters for reconstituting a persisted [`InviteToken`] entity from legacy params.
#[derive(Debug, Clone)]
pub struct InviteTokenParams {
    pub token: InviteTokenId,
    pub family_id: FamilyId,
    pub role: Role,
    pub permissions: Vec<Permission>,
    pub relationship: Option<Relationship>,
    pub created_by: MemberId,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
}

/// Parameters for reconstituting a persisted [`InviteToken`] entity from hashed DB storage.
#[derive(Debug, Clone)]
pub struct HashedInviteTokenParams {
    pub id: InviteId,
    pub token_hash: InviteTokenHash,
    pub family_id: FamilyId,
    pub role: Role,
    pub permissions: Vec<Permission>,
    pub relationship: Option<Relationship>,
    pub created_by: MemberId,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
}

/// The `Family` aggregate root.
///
/// Encapsulates the family profile, its member roster, and pending invite tokens.
/// All mutations emit domain events and enforce family-level invariants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Family {
    /// UUID v4 — random PK, avoids write hotspots on Aurora DSQL.
    id: FamilyId,
    /// Validated, trimmed family name (1–200 chars).
    name: FamilyName,
    /// ISO 4217 currency code for the family's default display currency.
    home_currency: CurrencyCode,
    /// All members of this family, including soft-deleted ones.
    members: Vec<FamilyMember>,
    /// All invite tokens issued for this family.
    invite_tokens: Vec<InviteToken>,
    created_at: DateTime<Utc>,
    /// Soft-delete timestamp. `None` means the family is active.
    deleted_at: Option<DateTime<Utc>>,
}

/// A member of a family. Child entity of `Family`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyMember {
    /// UUID v4 — random PK, avoids write hotspots on Aurora DSQL.
    id: MemberId,
    family_id: FamilyId,
    /// Cognito `sub` — the user's permanent identifier from JWT claims.
    user_id: UserId,
    /// Validated, trimmed display name (1–100 chars). Enforced by [`DisplayName`].
    display_name: DisplayName,
    role: Role,
    /// Custom capability flags (only used when `role == Role::Other`).
    permissions: Vec<Permission>,
    /// Optional family relationship label for any member (e.g., "Spouse", "Daughter", "Grandma", "Cousin").
    relationship: Option<Relationship>,
    joined_at: DateTime<Utc>,
    /// Soft-delete — never hard-delete members.
    deleted_at: Option<DateTime<Utc>>,
}

/// A one-time invite token that allows a new user to join the family.
///
/// Only `id` and `token_hash` (SHA-256) are persisted or emitted in events;
/// the raw bearer `token` is retained in-memory only on creation for the caller
/// and is excluded from `Serialize` and redacted in `Debug`.
#[derive(Clone, Serialize, Deserialize)]
pub struct InviteToken {
    /// UUID v4 — database primary key (`family_invite_tokens.id`).
    id: InviteId,
    /// SHA-256 hex digest of the bearer secret (`family_invite_tokens.token_hash`).
    token_hash: InviteTokenHash,
    /// In-memory bearer secret returned at invite creation; never serialized or persisted.
    #[serde(skip_serializing, default)]
    token: Option<InviteTokenId>,
    family_id: FamilyId,
    role: Role,
    /// Custom capability flags (only used when `role == Role::Other`).
    permissions: Vec<Permission>,
    /// Optional family relationship label (e.g., "Spouse", "Daughter", "Grandma", "Cousin").
    relationship: Option<Relationship>,
    /// References `family_members.id` of the member who created the invite.
    created_by: MemberId,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    used: bool,
}

impl Family {
    /// Reconstitutes a `Family` aggregate from persisted state.
    pub fn reconstitute(params: FamilyParams) -> Self {
        Self {
            id: params.id,
            name: params.name,
            home_currency: params.home_currency,
            members: params.members,
            invite_tokens: params.invite_tokens,
            created_at: params.created_at,
            deleted_at: params.deleted_at,
        }
    }

    /// Creates a new `Family` aggregate with the creator as the sole `Owner`.
    pub fn create(
        name: FamilyName,
        home_currency: CurrencyCode,
        owner_user_id: UserId,
        owner_display_name: DisplayName,
    ) -> Result<(Self, Vec<DomainEvent>), DomainError> {
        Self::create_at(
            name,
            home_currency,
            owner_user_id,
            owner_display_name,
            Utc::now(),
        )
    }

    /// Creates a new `Family` aggregate at an explicit timestamp `now`.
    pub fn create_at(
        name: FamilyName,
        home_currency: CurrencyCode,
        owner_user_id: UserId,
        owner_display_name: DisplayName,
        now: DateTime<Utc>,
    ) -> Result<(Self, Vec<DomainEvent>), DomainError> {
        let family_id = FamilyId::new_v4();
        let owner_member_id = MemberId::new_v4();
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
    /// bounded positive expiration duration.
    pub fn create_invite(
        &mut self,
        created_by_user_id: UserId,
        role: Role,
        permissions: Vec<Permission>,
        relationship: Option<Relationship>,
        ttl: InviteTtl,
    ) -> Result<(InviteToken, Vec<DomainEvent>), DomainError> {
        self.create_invite_at(
            created_by_user_id,
            role,
            permissions,
            relationship,
            ttl,
            Utc::now(),
        )
    }

    /// Creates a new invite token at an explicit timestamp `now`.
    pub fn create_invite_at(
        &mut self,
        created_by_user_id: UserId,
        role: Role,
        permissions: Vec<Permission>,
        relationship: Option<Relationship>,
        ttl: InviteTtl,
        now: DateTime<Utc>,
    ) -> Result<(InviteToken, Vec<DomainEvent>), DomainError> {
        self.ensure_active()?;
        let creator_member_id = self.authorize_owner(created_by_user_id)?;
        validate_invite_params(role, &permissions)?;
        let permissions = deduplicate_permissions(permissions);
        let expires_at =
            now.checked_add_signed(ttl.as_duration())
                .ok_or(DomainError::ArithmeticOverflow {
                    operation: "invite expiration",
                })?;

        let (invite, event) = build_invite_and_event(
            self.id,
            creator_member_id,
            role,
            permissions,
            relationship,
            now,
            expires_at,
        );
        self.invite_tokens.push(invite.clone());
        Ok((invite, vec![event]))
    }

    /// Accepts an invite token and adds (or reactivates) the user as a member.
    pub fn accept_invite(
        &mut self,
        token: InviteTokenId,
        user_id: UserId,
        display_name: DisplayName,
    ) -> Result<(FamilyMember, Vec<DomainEvent>), DomainError> {
        self.accept_invite_at(token, user_id, display_name, Utc::now())
    }

    /// Accepts an invite token at an explicit timestamp `now`.
    pub fn accept_invite_at(
        &mut self,
        token: InviteTokenId,
        user_id: UserId,
        display_name: DisplayName,
        now: DateTime<Utc>,
    ) -> Result<(FamilyMember, Vec<DomainEvent>), DomainError> {
        self.ensure_active()?;
        if self.has_active_member(user_id) {
            return Err(DomainError::MemberAlreadyExists { user_id });
        }
        let (role, permissions, relationship) = self.consume_invite(token, now)?;
        let member =
            self.upsert_member(user_id, display_name, role, permissions, relationship, now);

        let events = vec![DomainEvent::MemberJoined {
            family_id: self.id,
            member_id: member.id,
            user_id,
            role,
            occurred_at: now,
        }];
        Ok((member, events))
    }

    /// Changes a member's role and custom permissions.
    ///
    /// Only `Owner` can change roles; the Owner cannot be demoted, no second
    /// member can be promoted to `Owner`, and custom permissions are allowed only
    /// for `Role::Other`. No-op transitions return an empty event list.
    pub fn change_member_role(
        &mut self,
        requester_user_id: UserId,
        target_member_id: MemberId,
        new_role: Role,
        new_permissions: Vec<Permission>,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.change_member_role_at(
            requester_user_id,
            target_member_id,
            new_role,
            new_permissions,
            Utc::now(),
        )
    }

    /// Changes a member's role and custom permissions at an explicit timestamp `now`.
    pub fn change_member_role_at(
        &mut self,
        requester_user_id: UserId,
        target_member_id: MemberId,
        new_role: Role,
        new_permissions: Vec<Permission>,
        now: DateTime<Utc>,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.ensure_active()?;
        let requester_member_id = self.authorize_owner(requester_user_id)?;
        validate_role_permissions(new_role, &new_permissions)?;
        let new_permissions = deduplicate_permissions(new_permissions);
        let target = self.find_active_member_by_id_mut(target_member_id)?;
        validate_owner_role_transition(target.role, new_role)?;

        if target.role == new_role && target.permissions == new_permissions {
            return Ok(Vec::new());
        }

        let old_role = target.role;
        target.role = new_role;
        target.permissions = new_permissions.clone();

        Ok(vec![DomainEvent::MemberRoleChanged {
            family_id: self.id,
            member_id: target_member_id,
            old_role,
            new_role,
            permissions: new_permissions,
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
        self.remove_member_at(requester_user_id, target_member_id, Utc::now())
    }

    /// Soft-deletes a member at an explicit timestamp `now`.
    pub fn remove_member_at(
        &mut self,
        requester_user_id: UserId,
        target_member_id: MemberId,
        now: DateTime<Utc>,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.ensure_active()?;
        let requester_member_id = self.authorize_owner(requester_user_id)?;
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

    pub fn id(&self) -> FamilyId {
        self.id
    }

    pub fn name(&self) -> &FamilyName {
        &self.name
    }

    pub fn home_currency(&self) -> &CurrencyCode {
        &self.home_currency
    }

    pub fn members(&self) -> &[FamilyMember] {
        &self.members
    }

    pub fn active_members(&self) -> impl Iterator<Item = &FamilyMember> {
        self.members.iter().filter(|m| m.is_active())
    }

    pub fn invite_tokens(&self) -> &[InviteToken] {
        &self.invite_tokens
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn deleted_at(&self) -> Option<DateTime<Utc>> {
        self.deleted_at
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    fn ensure_active(&self) -> Result<(), DomainError> {
        if self.deleted_at.is_some() {
            return Err(DomainError::FamilyDeleted);
        }
        Ok(())
    }

    fn has_active_member(&self, user_id: UserId) -> bool {
        self.members
            .iter()
            .any(|m| m.user_id == user_id && m.is_active())
    }

    fn authorize_owner(&self, user_id: UserId) -> Result<MemberId, DomainError> {
        let member = self
            .members
            .iter()
            .find(|m| m.user_id == user_id && m.is_active())
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
            .find(|t| t.token_hash.matches_token(token))
            .ok_or(DomainError::InviteTokenNotFound)?;

        if invite.used {
            return Err(DomainError::TokenAlreadyUsed);
        }
        if invite.is_expired_at(now) {
            return Err(DomainError::TokenExpired);
        }

        invite.used = true;
        Ok((
            invite.role,
            invite.permissions.clone(),
            invite.relationship.clone(),
        ))
    }

    fn upsert_member(
        &mut self,
        user_id: UserId,
        display_name: DisplayName,
        role: Role,
        permissions: Vec<Permission>,
        relationship: Option<Relationship>,
        now: DateTime<Utc>,
    ) -> FamilyMember {
        if let Some(existing) = self.members.iter_mut().find(|m| m.user_id == user_id) {
            existing.display_name = display_name;
            existing.role = role;
            existing.permissions = permissions;
            existing.relationship = relationship;
            existing.joined_at = now;
            existing.deleted_at = None;
            return existing.clone();
        }

        let member = FamilyMember {
            id: MemberId::new_v4(),
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
        member
    }

    fn find_active_member_by_id_mut(
        &mut self,
        target_member_id: MemberId,
    ) -> Result<&mut FamilyMember, DomainError> {
        self.members
            .iter_mut()
            .find(|m| m.id == target_member_id && m.is_active())
            .ok_or(DomainError::MemberNotFound {
                member_id: target_member_id,
            })
    }
}

impl FamilyMember {
    /// Reconstitutes a `FamilyMember` entity from persisted state.
    pub fn reconstitute(params: FamilyMemberParams) -> Self {
        Self {
            id: params.id,
            family_id: params.family_id,
            user_id: params.user_id,
            display_name: params.display_name,
            role: params.role,
            permissions: params.permissions,
            relationship: params.relationship,
            joined_at: params.joined_at,
            deleted_at: params.deleted_at,
        }
    }

    pub fn id(&self) -> MemberId {
        self.id
    }

    pub fn family_id(&self) -> FamilyId {
        self.family_id
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn display_name(&self) -> &DisplayName {
        &self.display_name
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn permissions(&self) -> &[Permission] {
        &self.permissions
    }

    pub fn relationship(&self) -> Option<&Relationship> {
        self.relationship.as_ref()
    }

    pub fn joined_at(&self) -> DateTime<Utc> {
        self.joined_at
    }

    pub fn deleted_at(&self) -> Option<DateTime<Utc>> {
        self.deleted_at
    }

    /// Returns `true` if this member is currently active (not soft-deleted).
    pub fn is_active(&self) -> bool {
        self.deleted_at.is_none()
    }

    /// Evaluates whether this member has `perm`, respecting role rules and custom permissions.
    pub fn has_permission(&self, perm: Permission) -> bool {
        self.role.has_permission(&self.permissions, perm)
    }

    /// Evaluates any [`DomainAction`] for this member using their role and custom permissions.
    pub fn can_perform(&self, action: DomainAction) -> bool {
        self.role.can_perform(&self.permissions, action)
    }

    /// Asserts that this member may perform `action`.
    pub fn assert_can_perform(&self, action: DomainAction) -> Result<(), DomainError> {
        self.role.assert_can_perform(&self.permissions, action)
    }

    /// Returns `true` if this member can manage family membership (`Role::Owner` only).
    pub fn can_manage_family(&self) -> bool {
        self.role.can_manage_family()
    }

    /// Asserts that this member can manage family membership (`Role::Owner` only).
    pub fn assert_family_management(&self) -> Result<(), DomainError> {
        self.role.assert_family_management()
    }

    /// Returns `true` if this member can access debt/loan features.
    pub fn can_access_debt_features(&self) -> bool {
        self.role.can_access_debt_features(&self.permissions)
    }

    /// Asserts that this member may access debt/loan features.
    pub fn assert_debt_access(&self) -> Result<(), DomainError> {
        self.role.assert_debt_access(&self.permissions)
    }

    /// Returns `true` if this member can view family-level reports.
    pub fn can_view_family_reports(&self) -> bool {
        self.role.can_view_family_reports(&self.permissions)
    }

    /// Returns `true` if this member can manage own payment accounts.
    pub fn can_manage_accounts(&self) -> bool {
        self.role.can_manage_accounts(&self.permissions)
    }
}

impl InviteToken {
    /// Reconstitutes an `InviteToken` entity from raw token params (computes `token_hash`).
    pub fn reconstitute(params: InviteTokenParams) -> Self {
        Self {
            id: InviteId::new_v4(),
            token_hash: InviteTokenHash::from_token_id(params.token),
            token: Some(params.token),
            family_id: params.family_id,
            role: params.role,
            permissions: params.permissions,
            relationship: params.relationship,
            created_by: params.created_by,
            created_at: params.created_at,
            expires_at: params.expires_at,
            used: params.used,
        }
    }

    /// Reconstitutes an `InviteToken` entity from hashed DB persistence (without raw token).
    pub fn reconstitute_hashed(params: HashedInviteTokenParams) -> Self {
        Self {
            id: params.id,
            token_hash: params.token_hash,
            token: None,
            family_id: params.family_id,
            role: params.role,
            permissions: params.permissions,
            relationship: params.relationship,
            created_by: params.created_by,
            created_at: params.created_at,
            expires_at: params.expires_at,
            used: params.used,
        }
    }

    pub fn id(&self) -> InviteId {
        self.id
    }

    pub fn token_hash(&self) -> &InviteTokenHash {
        &self.token_hash
    }

    pub fn token(&self) -> InviteTokenId {
        self.token
            .unwrap_or_else(|| InviteTokenId::from_uuid(Uuid::nil()))
    }

    pub fn family_id(&self) -> FamilyId {
        self.family_id
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn permissions(&self) -> &[Permission] {
        &self.permissions
    }

    pub fn relationship(&self) -> Option<&Relationship> {
        self.relationship.as_ref()
    }

    pub fn created_by(&self) -> MemberId {
        self.created_by
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub fn is_used(&self) -> bool {
        self.used
    }

    /// Returns `true` if the token has expired at or before `now`.
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }
}

impl fmt::Debug for InviteToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InviteToken")
            .field("id", &self.id)
            .field("token_hash", &self.token_hash)
            .field("token", &"[REDACTED]")
            .field("family_id", &self.family_id)
            .field("role", &self.role)
            .field("permissions", &self.permissions)
            .field("relationship", &self.relationship)
            .field("created_by", &self.created_by)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .field("used", &self.used)
            .finish()
    }
}

fn build_invite_and_event(
    family_id: FamilyId,
    created_by: MemberId,
    role: Role,
    permissions: Vec<Permission>,
    relationship: Option<Relationship>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> (InviteToken, DomainEvent) {
    let invite_id = InviteId::new_v4();
    let token = InviteTokenId::new_v4();
    let token_hash = InviteTokenHash::from_token_id(token);
    let invite = InviteToken {
        id: invite_id,
        token_hash: token_hash.clone(),
        token: Some(token),
        family_id,
        role,
        permissions: permissions.clone(),
        relationship,
        created_by,
        created_at,
        expires_at,
        used: false,
    };
    let event = DomainEvent::MemberInvited {
        family_id,
        invite_id,
        invite_token_hash: token_hash,
        role,
        permissions,
        created_by,
        occurred_at: created_at,
    };
    (invite, event)
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
    validate_role_permissions(role, permissions)
}

fn validate_role_permissions(role: Role, permissions: &[Permission]) -> Result<(), DomainError> {
    if role != Role::Other && !permissions.is_empty() {
        return Err(DomainError::InvariantViolation {
            message: "Custom permissions can only be configured for Role::Other".to_string(),
        });
    }
    Ok(())
}

fn validate_owner_role_transition(old_role: Role, new_role: Role) -> Result<(), DomainError> {
    if old_role == Role::Owner && new_role != Role::Owner {
        return Err(DomainError::CannotRemoveOwner);
    }
    if new_role == Role::Owner && old_role != Role::Owner {
        return Err(DomainError::DuplicateOwner);
    }
    Ok(())
}

fn deduplicate_permissions(permissions: Vec<Permission>) -> Vec<Permission> {
    let mut deduped = Vec::with_capacity(permissions.len());
    for perm in permissions {
        if !deduped.contains(&perm) {
            deduped.push(perm);
        }
    }
    deduped
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_780_000_000, 0).unwrap()
    }

    fn make_family() -> (Family, UserId) {
        let owner_id = UserId::new_v4();
        let currency = CurrencyCode::try_new("USD").unwrap();
        let name = FamilyName::try_new("Test Family").unwrap();
        let owner_name = DisplayName::try_new("Alice").unwrap();
        let (family, _) = Family::create_at(name, currency, owner_id, owner_name, t0()).unwrap();
        (family, owner_id)
    }

    fn invite_and_accept_at(
        family: &mut Family,
        owner_id: UserId,
        user_id: UserId,
        name: &str,
        role: Role,
        now: DateTime<Utc>,
    ) -> FamilyMember {
        let (invite, _) = family
            .create_invite_at(
                owner_id,
                role,
                Vec::new(),
                None,
                InviteTtl::from_hours(48).unwrap(),
                now,
            )
            .unwrap();
        let (member, _) = family
            .accept_invite_at(
                invite.token(),
                user_id,
                DisplayName::try_new(name).unwrap(),
                now + chrono::Duration::minutes(5),
            )
            .unwrap();
        member
    }

    #[test]
    fn family_create_succeeds() {
        let (family, _) = make_family();
        assert_eq!(family.name().as_str(), "Test Family");
        assert_eq!(family.members().len(), 1);
        assert_eq!(family.members()[0].role(), Role::Owner);
        assert!(family.members()[0].can_manage_family());
        assert!(family.members()[0].assert_family_management().is_ok());
        assert!(!family.is_deleted());
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
        let member_user_id = UserId::new_v4();
        let member = invite_and_accept_at(
            &mut family,
            owner_id,
            member_user_id,
            "Bob",
            Role::Member,
            t0(),
        );
        assert!(!member.can_manage_family());
        assert!(matches!(
            member.assert_family_management(),
            Err(DomainError::InsufficientRole { role: Role::Member })
        ));

        let result = family.create_invite_at(
            member_user_id,
            Role::Child,
            Vec::new(),
            None,
            InviteTtl::from_hours(24).unwrap(),
            t0(),
        );
        assert!(matches!(
            result,
            Err(DomainError::InsufficientRole { role: Role::Member })
        ));
    }

    #[test]
    fn invite_for_owner_role_is_rejected() {
        let (mut family, owner_id) = make_family();
        let result = family.create_invite_at(
            owner_id,
            Role::Owner,
            Vec::new(),
            None,
            InviteTtl::from_hours(24).unwrap(),
            t0(),
        );
        assert!(matches!(result, Err(DomainError::DuplicateOwner)));
    }

    #[test]
    fn custom_permissions_rejected_for_non_other_roles() {
        let (mut family, owner_id) = make_family();
        let result = family.create_invite_at(
            owner_id,
            Role::Member,
            vec![Permission::ViewLoans],
            None,
            InviteTtl::from_hours(24).unwrap(),
            t0(),
        );
        assert!(matches!(
            result,
            Err(DomainError::InvariantViolation { .. })
        ));
    }

    #[test]
    fn other_role_invite_deduplicates_permissions_and_propagates_fields() {
        let (mut family, owner_id) = make_family();
        let owner_member_id = family.members()[0].id();
        let perms = vec![
            Permission::ViewBudget,
            Permission::RecordOwnTransactions,
            Permission::ViewBudget,
        ];
        let expected_perms = vec![Permission::ViewBudget, Permission::RecordOwnTransactions];
        let relationship = Relationship::try_new("Grandma").unwrap();

        let (invite, _) = family
            .create_invite_at(
                owner_id,
                Role::Other,
                perms,
                Some(relationship.clone()),
                InviteTtl::from_hours(48).unwrap(),
                t0(),
            )
            .unwrap();
        assert_eq!(invite.created_by(), owner_member_id);
        assert_eq!(invite.permissions(), expected_perms.as_slice());
        assert_eq!(invite.relationship(), Some(&relationship));

        let (member, _) = family
            .accept_invite_at(
                invite.token(),
                UserId::new_v4(),
                DisplayName::try_new("Elena").unwrap(),
                t0() + chrono::Duration::minutes(1),
            )
            .unwrap();
        assert_eq!(member.permissions(), expected_perms.as_slice());
        assert_eq!(member.relationship(), Some(&relationship));
        assert!(member.has_permission(Permission::ViewBudget));
        assert!(!member.has_permission(Permission::ViewLoans));
        assert!(!member.can_access_debt_features());
        assert!(matches!(
            member.assert_debt_access(),
            Err(DomainError::InsufficientRole { role: Role::Other })
        ));
        assert!(matches!(
            member.assert_can_perform(DomainAction::ManageFamily),
            Err(DomainError::InsufficientRole { role: Role::Other })
        ));
    }

    #[test]
    fn relationship_is_allowed_on_non_other_roles() {
        let (mut family, owner_id) = make_family();
        let relationship = Relationship::try_new("Spouse").unwrap();

        let (invite, _) = family
            .create_invite_at(
                owner_id,
                Role::Member,
                Vec::new(),
                Some(relationship.clone()),
                InviteTtl::from_hours(48).unwrap(),
                t0(),
            )
            .unwrap();
        let (member, _) = family
            .accept_invite_at(
                invite.token(),
                UserId::new_v4(),
                DisplayName::try_new("Partner").unwrap(),
                t0() + chrono::Duration::minutes(1),
            )
            .unwrap();

        assert_eq!(member.role(), Role::Member);
        assert_eq!(member.relationship(), Some(&relationship));
    }

    #[test]
    fn duplicate_active_member_is_rejected_without_consuming_token() {
        let (mut family, owner_id) = make_family();
        let bob_id = UserId::new_v4();
        let _bob = invite_and_accept_at(&mut family, owner_id, bob_id, "Bob", Role::Member, t0());

        let (invite, _) = family
            .create_invite_at(
                owner_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(48).unwrap(),
                t0(),
            )
            .unwrap();

        let err = family
            .accept_invite_at(
                invite.token(),
                bob_id,
                DisplayName::try_new("Bob Again").unwrap(),
                t0() + chrono::Duration::minutes(10),
            )
            .unwrap_err();
        assert_eq!(err, DomainError::MemberAlreadyExists { user_id: bob_id });

        let stored = family
            .invite_tokens()
            .iter()
            .find(|t| t.token() == invite.token())
            .unwrap();
        assert!(!stored.is_used());

        let carol = family.accept_invite_at(
            invite.token(),
            UserId::new_v4(),
            DisplayName::try_new("Carol").unwrap(),
            t0() + chrono::Duration::minutes(15),
        );
        assert!(carol.is_ok());
    }

    #[test]
    fn soft_deleted_member_is_reactivated_preserving_member_id() {
        let (mut family, owner_id) = make_family();
        let bob_id = UserId::new_v4();
        let bob = invite_and_accept_at(&mut family, owner_id, bob_id, "Bob", Role::Member, t0());
        let original_member_id = bob.id();

        family
            .remove_member_at(
                owner_id,
                original_member_id,
                t0() + chrono::Duration::hours(1),
            )
            .unwrap();

        let (invite, _) = family
            .create_invite_at(
                owner_id,
                Role::Other,
                vec![Permission::ViewBudget],
                Some(Relationship::try_new("Sibling").unwrap()),
                InviteTtl::from_hours(24).unwrap(),
                t0() + chrono::Duration::hours(2),
            )
            .unwrap();

        let (rejoined, events) = family
            .accept_invite_at(
                invite.token(),
                bob_id,
                DisplayName::try_new("Robert").unwrap(),
                t0() + chrono::Duration::hours(3),
            )
            .unwrap();

        assert_eq!(rejoined.id(), original_member_id);
        assert!(rejoined.is_active());
        assert_eq!(rejoined.role(), Role::Other);
        assert_eq!(rejoined.display_name().as_str(), "Robert");
        assert_eq!(rejoined.permissions(), &[Permission::ViewBudget]);
        assert_eq!(family.active_members().count(), 2);
        assert_eq!(family.members().len(), 2);
        assert!(matches!(
            events.as_slice(),
            [DomainEvent::MemberJoined { member_id, .. }] if *member_id == original_member_id
        ));
    }

    #[test]
    fn expired_and_unknown_invite_tokens_are_rejected() {
        let (mut family, owner_id) = make_family();
        let unknown_err = family
            .accept_invite_at(
                InviteTokenId::new_v4(),
                UserId::new_v4(),
                DisplayName::try_new("Carol").unwrap(),
                t0(),
            )
            .unwrap_err();
        assert_eq!(unknown_err, DomainError::InviteTokenNotFound);

        let (invite, _) = family
            .create_invite_at(
                owner_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(1).unwrap(),
                t0(),
            )
            .unwrap();

        // Exact boundary expires_at == now is treated as expired
        let exact_err = family
            .accept_invite_at(
                invite.token(),
                UserId::new_v4(),
                DisplayName::try_new("Carol").unwrap(),
                invite.expires_at(),
            )
            .unwrap_err();
        assert_eq!(exact_err, DomainError::TokenExpired);

        let after_err = family
            .accept_invite_at(
                invite.token(),
                UserId::new_v4(),
                DisplayName::try_new("Carol").unwrap(),
                invite.expires_at() + chrono::Duration::seconds(1),
            )
            .unwrap_err();
        assert_eq!(after_err, DomainError::TokenExpired);
    }

    #[test]
    fn used_token_cannot_be_reused() {
        let (mut family, owner_id) = make_family();
        let (invite, _) = family
            .create_invite_at(
                owner_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(48).unwrap(),
                t0(),
            )
            .unwrap();

        family
            .accept_invite_at(
                invite.token(),
                UserId::new_v4(),
                DisplayName::try_new("Bob").unwrap(),
                t0() + chrono::Duration::minutes(1),
            )
            .unwrap();
        let result = family.accept_invite_at(
            invite.token(),
            UserId::new_v4(),
            DisplayName::try_new("Carol").unwrap(),
            t0() + chrono::Duration::minutes(2),
        );

        assert!(matches!(result, Err(DomainError::TokenAlreadyUsed)));
    }

    #[test]
    fn soft_deleted_family_blocks_all_mutations() {
        let (mut family, owner_id) = make_family();
        let member = invite_and_accept_at(
            &mut family,
            owner_id,
            UserId::new_v4(),
            "Bob",
            Role::Member,
            t0(),
        );
        let (pending_invite, _) = family
            .create_invite_at(
                owner_id,
                Role::Child,
                Vec::new(),
                None,
                InviteTtl::from_hours(24).unwrap(),
                t0(),
            )
            .unwrap();

        let deleted_family = Family::reconstitute(FamilyParams {
            id: family.id(),
            name: family.name().clone(),
            home_currency: family.home_currency().clone(),
            members: family.members().to_vec(),
            invite_tokens: family.invite_tokens().to_vec(),
            created_at: family.created_at(),
            deleted_at: Some(t0() + chrono::Duration::hours(1)),
        });
        assert!(deleted_family.is_deleted());

        let mut f1 = deleted_family.clone();
        assert_eq!(
            f1.create_invite_at(
                owner_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(1).unwrap(),
                t0(),
            )
            .unwrap_err(),
            DomainError::FamilyDeleted
        );
        assert_eq!(
            f1.accept_invite_at(
                pending_invite.token(),
                UserId::new_v4(),
                DisplayName::try_new("Carol").unwrap(),
                t0(),
            )
            .unwrap_err(),
            DomainError::FamilyDeleted
        );
        assert_eq!(
            f1.change_member_role_at(owner_id, member.id(), Role::Child, Vec::new(), t0())
                .unwrap_err(),
            DomainError::FamilyDeleted
        );
        assert_eq!(
            f1.remove_member_at(owner_id, member.id(), t0())
                .unwrap_err(),
            DomainError::FamilyDeleted
        );
    }

    #[test]
    fn owner_cannot_be_removed() {
        let (mut family, owner_id) = make_family();
        let owner_member_id = family.members()[0].id();

        let result = family.remove_member_at(owner_id, owner_member_id, t0());
        assert!(matches!(result, Err(DomainError::CannotRemoveOwner)));
    }

    #[test]
    fn non_member_caller_returns_user_not_in_family_and_missing_target_returns_member_not_found() {
        let (mut family, owner_id) = make_family();
        let outsider_user_id = UserId::new_v4();
        let unknown_member_id = MemberId::new_v4();

        let invite_err = family
            .create_invite_at(
                outsider_user_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(24).unwrap(),
                t0(),
            )
            .unwrap_err();
        assert_eq!(
            invite_err,
            DomainError::UserNotInFamily {
                user_id: outsider_user_id
            }
        );

        let remove_outsider_err = family
            .remove_member_at(outsider_user_id, unknown_member_id, t0())
            .unwrap_err();
        assert_eq!(
            remove_outsider_err,
            DomainError::UserNotInFamily {
                user_id: outsider_user_id
            }
        );

        let missing_target_err = family
            .remove_member_at(owner_id, unknown_member_id, t0())
            .unwrap_err();
        assert_eq!(
            missing_target_err,
            DomainError::MemberNotFound {
                member_id: unknown_member_id
            }
        );
    }

    #[test]
    fn change_member_role_updates_other_permissions_and_skips_noop_events() {
        let (mut family, owner_id) = make_family();
        let owner_member_id = family.members()[0].id();
        let bob = invite_and_accept_at(
            &mut family,
            owner_id,
            UserId::new_v4(),
            "Bob",
            Role::Member,
            t0(),
        );

        // No-op Owner -> Owner emits zero events
        let owner_noop = family
            .change_member_role_at(owner_id, owner_member_id, Role::Owner, Vec::new(), t0())
            .unwrap();
        assert!(owner_noop.is_empty());

        // Custom permissions rejected for non-Other
        let bad_perms = family
            .change_member_role_at(
                owner_id,
                bob.id(),
                Role::Child,
                vec![Permission::ViewLoans],
                t0(),
            )
            .unwrap_err();
        assert!(matches!(bad_perms, DomainError::InvariantViolation { .. }));

        // Transition Member -> Other with deduplicated permissions
        let events = family
            .change_member_role_at(
                owner_id,
                bob.id(),
                Role::Other,
                vec![Permission::ViewLoans, Permission::ViewLoans],
                t0() + chrono::Duration::minutes(10),
            )
            .unwrap();
        assert!(matches!(
            events.as_slice(),
            [DomainEvent::MemberRoleChanged {
                old_role: Role::Member,
                new_role: Role::Other,
                permissions,
                ..
            }] if permissions == &[Permission::ViewLoans]
        ));

        // Repeat exact same role + permissions emits zero events
        let noop_events = family
            .change_member_role_at(
                owner_id,
                bob.id(),
                Role::Other,
                vec![Permission::ViewLoans],
                t0() + chrono::Duration::minutes(11),
            )
            .unwrap();
        assert!(noop_events.is_empty());
    }

    #[test]
    fn owner_cannot_change_own_role_away_from_owner() {
        let (mut family, owner_id) = make_family();
        let owner_member_id = family.members()[0].id();

        let result =
            family.change_member_role_at(owner_id, owner_member_id, Role::Member, Vec::new(), t0());
        assert!(matches!(result, Err(DomainError::CannotRemoveOwner)));
    }

    #[test]
    fn promoting_another_member_to_owner_is_rejected() {
        let (mut family, owner_id) = make_family();
        let member = invite_and_accept_at(
            &mut family,
            owner_id,
            UserId::new_v4(),
            "Bob",
            Role::Member,
            t0(),
        );

        let result =
            family.change_member_role_at(owner_id, member.id(), Role::Owner, Vec::new(), t0());
        assert!(matches!(result, Err(DomainError::DuplicateOwner)));
    }

    #[test]
    fn member_soft_delete_sets_deleted_at() {
        let (mut family, owner_id) = make_family();
        let member = invite_and_accept_at(
            &mut family,
            owner_id,
            UserId::new_v4(),
            "Bob",
            Role::Member,
            t0(),
        );

        family
            .remove_member_at(owner_id, member.id(), t0() + chrono::Duration::minutes(10))
            .unwrap();

        let removed = family
            .members()
            .iter()
            .find(|m| m.id() == member.id())
            .unwrap();
        assert!(removed.deleted_at().is_some());
        assert!(!removed.is_active());
    }

    #[test]
    fn family_create_emits_two_events() {
        let owner_id = UserId::new_v4();
        let currency = CurrencyCode::try_new("MDL").unwrap();
        let name = FamilyName::try_new("My Family").unwrap();
        let owner_name = DisplayName::try_new("Ana").unwrap();
        let (_, events) =
            Family::create_at(name.clone(), currency.clone(), owner_id, owner_name, t0()).unwrap();

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

    #[test]
    fn invite_token_hashes_secret_redacts_debug_and_reconstitutes_from_hash() {
        let (mut family, owner_id) = make_family();
        let (invite, events) = family
            .create_invite_at(
                owner_id,
                Role::Member,
                Vec::new(),
                None,
                InviteTtl::from_hours(24).unwrap(),
                t0(),
            )
            .unwrap();

        let raw_token = invite.token();
        let debug_str = format!("{invite:?}");
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains(&raw_token.to_string()));

        let json_str = serde_json::to_string(&invite).unwrap();
        assert!(!json_str.contains(&raw_token.to_string()));
        assert!(json_str.contains(invite.token_hash().as_str()));

        assert!(matches!(
            events.as_slice(),
            [DomainEvent::MemberInvited {
                invite_id,
                invite_token_hash,
                ..
            }] if *invite_id == invite.id() && invite_token_hash == invite.token_hash()
        ));

        let hashed_only = InviteToken::reconstitute_hashed(HashedInviteTokenParams {
            id: invite.id(),
            token_hash: invite.token_hash().clone(),
            family_id: invite.family_id(),
            role: invite.role(),
            permissions: invite.permissions().to_vec(),
            relationship: invite.relationship().cloned(),
            created_by: invite.created_by(),
            created_at: invite.created_at(),
            expires_at: invite.expires_at(),
            used: false,
        });
        assert_eq!(hashed_only.token(), InviteTokenId::from_uuid(Uuid::nil()));

        let mut rehydrated = Family::reconstitute(FamilyParams {
            id: family.id(),
            name: family.name().clone(),
            home_currency: family.home_currency().clone(),
            members: family.members().to_vec(),
            invite_tokens: vec![hashed_only],
            created_at: family.created_at(),
            deleted_at: None,
        });

        let wrong_err = rehydrated
            .accept_invite_at(
                InviteTokenId::new_v4(),
                UserId::new_v4(),
                DisplayName::try_new("Wrong").unwrap(),
                t0() + chrono::Duration::minutes(5),
            )
            .unwrap_err();
        assert_eq!(wrong_err, DomainError::InviteTokenNotFound);

        let (joined, _) = rehydrated
            .accept_invite_at(
                raw_token,
                UserId::new_v4(),
                DisplayName::try_new("Bob").unwrap(),
                t0() + chrono::Duration::minutes(5),
            )
            .unwrap();
        assert_eq!(joined.role(), Role::Member);
    }
}
