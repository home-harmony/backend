//! Wire DTOs for Identity & Family HTTP endpoints.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use domain::{
    entities::{Family, FamilyMember, InviteToken},
    ports::UserFamilySummary,
    value_objects::{
        CurrencyCode, DisplayName, FamilyId, FamilyName, InviteTokenId, MemberId, Permission,
        Relationship, Role, UserId,
    },
};

/// Request body for `POST /families`.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateFamilyRequest {
    pub name: String,
    pub home_currency: String,
    pub owner_display_name: String,
}

/// Request body for `PUT /families/{id}`.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateFamilyRequest {
    pub name: String,
    pub home_currency: String,
}

/// Request body for `POST /families/{id}/invites`.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateInviteRequest {
    pub role: Role,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub relationship: Option<String>,
    #[serde(default)]
    pub ttl_hours: Option<i64>,
}

/// Request body for `POST /families/{id}/members`.
#[derive(Debug, Clone, Deserialize)]
pub struct AcceptInviteRequest {
    pub token: InviteTokenId,
    pub display_name: String,
}

/// Request body for `PATCH /families/{id}/members/{mid}/role`.
#[derive(Debug, Clone, Deserialize)]
pub struct ChangeMemberRoleRequest {
    pub role: Role,
    #[serde(default)]
    pub permissions: Vec<Permission>,
}

/// Response body for `POST /families` (`201 Created`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateFamilyResponse {
    pub id: FamilyId,
    pub name: FamilyName,
    pub home_currency: CurrencyCode,
    pub owner_member_id: MemberId,
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /families/{id}` and `PUT /families/{id}` (`200 OK`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FamilyResponse {
    pub id: FamilyId,
    pub name: FamilyName,
    pub home_currency: CurrencyCode,
    pub created_at: DateTime<Utc>,
}

/// Response body for `POST /families/{id}/invites` (`201 Created`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InviteResponse {
    pub token: InviteTokenId,
    pub family_id: FamilyId,
    pub role: Role,
    pub permissions: Vec<Permission>,
    pub relationship: Option<Relationship>,
    pub created_by: MemberId,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Response body for `POST /families/{id}/members`, `GET /families/{id}/members`,
/// and `PATCH /families/{id}/members/{mid}/role`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemberResponse {
    pub id: MemberId,
    pub family_id: FamilyId,
    pub user_id: UserId,
    pub display_name: DisplayName,
    pub role: Role,
    pub permissions: Vec<Permission>,
    pub relationship: Option<Relationship>,
    pub joined_at: DateTime<Utc>,
}

/// Response body for `GET /families/{id}/members` (`200 OK`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListMembersResponse {
    pub items: Vec<MemberResponse>,
}

/// Item in `GET /profile/families` response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserFamilySummaryDto {
    pub family_id: FamilyId,
    pub name: FamilyName,
    pub home_currency: CurrencyCode,
    pub role: Role,
    pub display_name: DisplayName,
}

/// Response body for `GET /profile/families` (`200 OK`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListUserFamiliesResponse {
    pub items: Vec<UserFamilySummaryDto>,
}

impl From<&Family> for FamilyResponse {
    fn from(family: &Family) -> Self {
        Self {
            id: family.id(),
            name: family.name().clone(),
            home_currency: family.home_currency().clone(),
            created_at: family.created_at(),
        }
    }
}

impl From<&InviteToken> for InviteResponse {
    fn from(invite: &InviteToken) -> Self {
        Self {
            token: invite.token(),
            family_id: invite.family_id(),
            role: invite.role(),
            permissions: invite.permissions().to_vec(),
            relationship: invite.relationship().cloned(),
            created_by: invite.created_by(),
            created_at: invite.created_at(),
            expires_at: invite.expires_at(),
        }
    }
}

impl From<&FamilyMember> for MemberResponse {
    fn from(member: &FamilyMember) -> Self {
        Self {
            id: member.id(),
            family_id: member.family_id(),
            user_id: member.user_id(),
            display_name: member.display_name().clone(),
            role: member.role(),
            permissions: member.permissions().to_vec(),
            relationship: member.relationship().cloned(),
            joined_at: member.joined_at(),
        }
    }
}

impl From<UserFamilySummary> for UserFamilySummaryDto {
    fn from(summary: UserFamilySummary) -> Self {
        Self {
            family_id: summary.family_id,
            name: summary.name,
            home_currency: summary.home_currency,
            role: summary.role,
            display_name: summary.display_name,
        }
    }
}
