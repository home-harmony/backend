//! Pure domain repository and event-publishing ports (RPITIT static dispatch).

use std::future::Future;

use crate::entities::Family;
use crate::errors::DomainError;
use crate::events::DomainEvent;
use crate::value_objects::{
    CurrencyCode, DisplayName, FamilyId, FamilyName, Permission, Role, UserId,
};

/// Resolved active membership permissions for an authenticated caller in a family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberAccess {
    pub role: Role,
    pub permissions: Vec<Permission>,
}

/// Summary row returned when listing all active families a user belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFamilySummary {
    pub family_id: FamilyId,
    pub name: FamilyName,
    pub home_currency: CurrencyCode,
    pub role: Role,
    pub display_name: DisplayName,
}

/// Read-only port resolving active (`deleted_at IS NULL`) membership for `(family_id, user_id)`.
pub trait MembershipReader: Send + Sync {
    /// Looks up the caller's active role and permissions within `family_id`.
    fn find_active_membership(
        &self,
        family_id: FamilyId,
        user_id: UserId,
    ) -> impl Future<Output = Result<Option<MemberAccess>, DomainError>> + Send;
}

/// Persistence port for the [`Family`] aggregate root.
pub trait FamilyRepository: Send + Sync {
    /// Loads a `Family` aggregate (including `deleted_at`, all active/soft-deleted members,
    /// and invite tokens) by `family_id`.
    ///
    /// Returns `Ok(None)` only when no `family_families` row exists (`DomainError::FamilyNotFound`),
    /// allowing aggregate methods to surface `DomainError::FamilyDeleted` (`410 Gone`) when
    /// `deleted_at` is set and reactivate soft-deleted members (`FM-16`).
    fn find_by_id(
        &self,
        family_id: FamilyId,
    ) -> impl Future<Output = Result<Option<Family>, DomainError>> + Send;

    /// Lists all active (`deleted_at IS NULL`) family workspaces where `user_id` holds an active membership.
    fn list_for_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<UserFamilySummary>, DomainError>> + Send;

    /// Persists the `Family` aggregate state atomically via idempotent upserts (`INSERT ... ON CONFLICT DO UPDATE`,
    /// never `DELETE`) wrapped in OCC retries on Aurora DSQL.
    fn save(&self, family: &Family) -> impl Future<Output = Result<(), DomainError>> + Send;
}

/// Outbound port for publishing committed [`DomainEvent`]s to EventBridge (ADR-0017).
pub trait EventPublisher: Send + Sync {
    /// Publishes a batch of domain events after database persistence commits.
    fn publish(
        &self,
        events: &[DomainEvent],
    ) -> impl Future<Output = Result<(), DomainError>> + Send;
}
