//! Pure domain repository and event-publishing ports (RPITIT static dispatch).

use std::future::Future;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::entities::Family;
use crate::errors::DomainError;
use crate::events::DomainEvent;
use crate::value_objects::{
    AccountId, AccountKind, CurrencyCode, FamilyId, FamilyName, InviteTokenId, MemberDisplayName,
    MemberId, Permission, PositiveMoney, Role, UserId,
};

/// Resolved active membership permissions for an authenticated caller in a family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberAccess {
    pub member_id: MemberId,
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
    pub member_id: MemberId,
    pub role: Role,
    pub permissions: Vec<Permission>,
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

/// Read-only account metadata projection consumed by `ledger_service` when validating transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountAccess {
    pub account_id: AccountId,
    pub owner_member_id: MemberId,
    pub kind: AccountKind,
    pub currency: CurrencyCode,
    pub is_deleted: bool,
}

/// Read-only active ledger movement projection consumed by `accounts_service` to derive balances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerMovement {
    pub occurred_at: DateTime<Utc>,
    pub source_account_id: Option<AccountId>,
    pub destination_account_id: Option<AccountId>,
    pub amount: PositiveMoney,
    pub destination_amount: Option<PositiveMoney>,
}

/// Read-only port verifying account existence, ownership, kind, currency, and soft-delete state.
pub trait AccountAccessReader: Send + Sync {
    /// Looks up account metadata within `family_id`.
    fn find_account_access(
        &self,
        family_id: FamilyId,
        account_id: AccountId,
    ) -> impl Future<Output = Result<Option<AccountAccess>, DomainError>> + Send;
}

/// Read-only port listing active (`deleted_at IS NULL`) ledger movements affecting an account.
pub trait AccountLedgerDeltaReader: Send + Sync {
    /// Returns active movements referencing `account_id` where
    /// `occurred_at > after_exclusive` (if `Some`) and `occurred_at <= through_inclusive` (if `Some`).
    fn list_active_movements_after(
        &self,
        family_id: FamilyId,
        account_id: AccountId,
        after_exclusive: Option<DateTime<Utc>>,
        through_inclusive: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<Vec<LedgerMovement>, DomainError>> + Send;
}

/// Read-only port fetching the latest daily exchange rate (`1 quote = rate base`).
pub trait ExchangeRateReader: Send + Sync {
    /// Returns the latest rate converting `quote_currency` to `base_currency`, if available.
    fn find_latest_rate(
        &self,
        base_currency: &CurrencyCode,
        quote_currency: &CurrencyCode,
    ) -> impl Future<Output = Result<Option<Decimal>, DomainError>> + Send;
}

/// Read-only port resolving a family workspace's `HomeDisplayCurrency`.
pub trait FamilyCurrencyReader: Send + Sync {
    /// Returns the active family's `home_currency` (`None` if missing).
    fn find_home_currency(
        &self,
        family_id: FamilyId,
    ) -> impl Future<Output = Result<Option<CurrencyCode>, DomainError>> + Send;
}
