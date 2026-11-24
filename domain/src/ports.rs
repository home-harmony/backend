//! Pure domain repository and event-publishing ports (RPITIT static dispatch).

use std::future::Future;

use crate::entities::Family;
use crate::errors::DomainError;
use crate::events::DomainEvent;
use crate::value_objects::{
    CurrencyCode, MemberDisplayName, FamilyId, FamilyName, InviteTokenId, Permission, Role, UserId,
};

/// Resolved active membership permissions for an authenticated caller in a family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberAccess {
    pub role: Role,
    pub permissions: Vec<Permission>,
}

/// Summary of a family workspace in a [`UserFamilySummary`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilySummary {
    pub id: FamilyId,
    pub name: FamilyName,
    pub home_currency: CurrencyCode,
}

/// Summary of the caller's active membership in a [`UserFamilySummary`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSummary {
    pub role: Role,
    pub display_name: MemberDisplayName,
}

/// Summary row returned when listing all active families a user belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFamilySummary {
    pub family: FamilySummary,
    pub membership: MemberSummary,
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

/// Which invite tokens an atomic [`FamilyRepository::update`] loads into the aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenScope {
    /// No invite tokens are loaded (newly created tokens are still persisted).
    None,
    /// Only the given token (scoped to the family) is loaded, if it exists.
    One(InviteTokenId),
}

/// Result of a committed [`FamilyRepository::update`]: the closure's value plus its events.
#[derive(Debug, Clone, PartialEq)]
pub struct Updated<T> {
    pub value: T,
    pub events: Vec<DomainEvent>,
}

/// Persistence port for the [`Family`] aggregate root.
pub trait FamilyRepository: Send + Sync {
    /// Read-only load of a `Family` aggregate (including `deleted_at` and all
    /// active/soft-deleted members, but **no invite tokens**) by `family_id`.
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

    /// Inserts a brand-new aggregate (family, members, tokens) in one transaction.
    /// Insert-only: never upserts, never overwrites an existing family.
    fn insert_new(&self, family: &Family) -> impl Future<Output = Result<(), DomainError>> + Send;

    /// Atomic read → mutate → write of one family.
    ///
    /// Locks the family, loads it (tokens per `scope`), runs `op`, and persists only the
    /// rows `op` changed, all inside one transaction. `op` may run more than once (OCC
    /// retry re-reads and re-runs it), so it must be pure with respect to anything
    /// outside `&mut Family`. A returned `Err` rolls back and is never retried. Events in
    /// the result come from the committed attempt only.
    ///
    /// Returns `DomainError::FamilyNotFound` when the family row does not exist.
    fn update<T, F>(
        &self,
        family_id: FamilyId,
        scope: TokenScope,
        op: F,
    ) -> impl Future<Output = Result<Updated<T>, DomainError>> + Send
    where
        T: Send,
        F: Fn(&mut Family) -> Result<(T, Vec<DomainEvent>), DomainError> + Send + Sync;
}

/// Outbound port for publishing committed [`DomainEvent`]s to EventBridge (ADR-0017).
pub trait EventPublisher: Send + Sync {
    /// Publishes a batch of domain events after database persistence commits.
    fn publish(
        &self,
        events: &[DomainEvent],
    ) -> impl Future<Output = Result<(), DomainError>> + Send;
}
