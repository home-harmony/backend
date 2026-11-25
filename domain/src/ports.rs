//! Domain ports (hexagonal boundaries) implemented by `infrastructure/`.

use std::future::Future;

use crate::value_objects::{FamilyId, MemberId, Permission, Role, UserId};

/// Active, non-deleted membership record resolved for `(family_id, user_id)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveFamilyMembership {
    pub member_id: MemberId,
    pub family_id: FamilyId,
    pub user_id: UserId,
    pub role: Role,
    pub permissions: Vec<Permission>,
}

/// Read-only port for verifying a user's active membership in a family workspace.
pub trait FamilyMembershipReader: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn find_active_membership(
        &self,
        family_id: FamilyId,
        user_id: UserId,
    ) -> impl Future<Output = Result<Option<ActiveFamilyMembership>, Self::Error>> + Send;
}
