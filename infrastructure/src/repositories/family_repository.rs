//! Aurora DSQL persistence adapter for [`FamilyRepository`] and [`MembershipReader`].

use aurora_dsql_sqlx_connector::{OCCRetryConfig, retry_on_occ};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

use domain::{
    entities::{
        Family, FamilyMember, FamilyMemberParams, FamilyParams, InviteToken, InviteTokenParams,
    },
    errors::DomainError,
    events::DomainEvent,
    ports::{
        FamilyRepository, FamilySummary, MemberAccess, MemberSummary, MembershipReader, TokenScope,
        Updated, UserFamilySummary,
    },
    value_objects::{
        CurrencyCode, MemberDisplayName, FamilyId, FamilyName, InviteTokenId, MemberId, Permission,
        Relationship, Role, UserId,
    },
};

use super::family_writes::{
    Snapshot, TxFailure, apply_changes, ensure_change_budget, insert_aggregate,
};

/// Aurora DSQL implementation of [`FamilyRepository`] and [`MembershipReader`].
#[derive(Debug, Clone)]
pub struct DsqlFamilyRepository {
    pool: PgPool,
    occ_config: OCCRetryConfig,
}

struct MembershipRow {
    role: String,
    permissions: Value,
}

struct FamilyRow {
    id: Uuid,
    name: String,
    home_currency: String,
    created_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

struct MemberRow {
    id: Uuid,
    family_id: Uuid,
    user_id: Uuid,
    display_name: String,
    role: String,
    permissions: Value,
    relationship: Option<String>,
    joined_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

struct InviteTokenRow {
    token: Uuid,
    family_id: Uuid,
    role: String,
    permissions: Value,
    relationship: Option<String>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    used: bool,
}

struct UserFamilyRow {
    family_id: Uuid,
    name: String,
    home_currency: String,
    role: String,
    display_name: String,
}

impl DsqlFamilyRepository {
    /// Creates a repository using default Aurora DSQL OCC retry configuration.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            occ_config: OCCRetryConfig::default(),
        }
    }

    /// Creates a repository with an explicit Aurora DSQL OCC retry configuration.
    #[must_use]
    pub fn with_occ_config(pool: PgPool, occ_config: OCCRetryConfig) -> Self {
        Self { pool, occ_config }
    }
}

impl MembershipReader for DsqlFamilyRepository {
    async fn find_active_membership(
        &self,
        family_id: FamilyId,
        user_id: UserId,
    ) -> Result<Option<MemberAccess>, DomainError> {
        let mut tx = self
            .pool
            .begin_with("BEGIN READ ONLY")
            .await
            .map_err(|err| internal_err("Failed to begin read-only membership tx", err))?;

        let row = sqlx::query_as!(
            MembershipRow,
            "SELECT role, permissions FROM family_members \
             WHERE family_id = $1 AND user_id = $2 AND deleted_at IS NULL",
            family_id.into_inner(),
            user_id.into_inner(),
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|err| internal_err("Failed to query active family membership", err))?;

        tx.commit()
            .await
            .map_err(|err| internal_err("Failed to commit read-only membership tx", err))?;

        row.map(map_member_access).transpose()
    }
}

impl FamilyRepository for DsqlFamilyRepository {
    async fn find_by_id(&self, family_id: FamilyId) -> Result<Option<Family>, DomainError> {
        let mut tx = self
            .pool
            .begin_with("BEGIN READ ONLY")
            .await
            .map_err(|err| internal_err("Failed to begin read-only family tx", err))?;

        let raw_id = family_id.into_inner();
        let Some(family_row) = fetch_family_row(&mut tx, raw_id)
            .await
            .map_err(|err| internal_err("Failed to query family_families row", err))?
        else {
            tx.commit()
                .await
                .map_err(|err| internal_err("Failed to commit read-only family tx", err))?;
            return Ok(None);
        };
        let member_rows = fetch_member_rows(&mut tx, raw_id)
            .await
            .map_err(|err| internal_err("Failed to query family_members rows", err))?;

        tx.commit()
            .await
            .map_err(|err| internal_err("Failed to commit read-only family tx", err))?;

        reconstitute_family(family_row, member_rows, Vec::new()).map(Some)
    }

    async fn list_for_user(&self, user_id: UserId) -> Result<Vec<UserFamilySummary>, DomainError> {
        let mut tx = self
            .pool
            .begin_with("BEGIN READ ONLY")
            .await
            .map_err(|err| internal_err("Failed to begin read-only user families tx", err))?;

        let rows = sqlx::query_as!(
            UserFamilyRow,
            "SELECT f.id AS family_id, f.name, f.home_currency, m.role, m.display_name \
             FROM family_members m \
             INNER JOIN family_families f ON f.id = m.family_id \
             WHERE m.user_id = $1 AND m.deleted_at IS NULL AND f.deleted_at IS NULL \
             ORDER BY f.created_at ASC, f.id ASC",
            user_id.into_inner(),
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|err| internal_err("Failed to list user families", err))?;

        tx.commit()
            .await
            .map_err(|err| internal_err("Failed to commit read-only user families tx", err))?;

        rows.into_iter().map(map_user_family_summary).collect()
    }

    async fn insert_new(&self, family: &Family) -> Result<(), DomainError> {
        let snapshot = Snapshot::capture(family)?;
        retry_on_occ(&self.occ_config, || attempt_insert(&self.pool, &snapshot))
            .await
            .map_err(|err| internal_err("Failed to insert Family aggregate", err))?
    }

    async fn update<T, F>(
        &self,
        family_id: FamilyId,
        scope: TokenScope,
        op: F,
    ) -> Result<Updated<T>, DomainError>
    where
        T: Send,
        F: Fn(&mut Family) -> Result<(T, Vec<DomainEvent>), DomainError> + Send + Sync,
    {
        retry_on_occ(&self.occ_config, || {
            attempt_update(&self.pool, family_id, scope, &op)
        })
        .await
        .map_err(|err| internal_err("Failed to update Family aggregate", err))?
    }
}

/// One `insert_new` attempt; a domain verdict is `Ok(Err(..))` so OCC retry never replays it.
async fn attempt_insert(
    pool: &PgPool,
    snapshot: &Snapshot,
) -> Result<Result<(), DomainError>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = insert_aggregate(&mut tx, snapshot).await;
    finish_tx(tx, result).await
}

/// One `update` attempt: lock -> load -> `op` -> diff-write -> commit, all in one transaction.
async fn attempt_update<T, F>(
    pool: &PgPool,
    family_id: FamilyId,
    scope: TokenScope,
    op: &F,
) -> Result<Result<Updated<T>, DomainError>, sqlx::Error>
where
    F: Fn(&mut Family) -> Result<(T, Vec<DomainEvent>), DomainError>,
{
    let mut tx = pool.begin().await?;
    let result = run_update(&mut tx, family_id, scope, op).await;
    finish_tx(tx, result).await
}

async fn run_update<T, F>(
    conn: &mut PgConnection,
    family_id: FamilyId,
    scope: TokenScope,
    op: &F,
) -> Result<Updated<T>, TxFailure>
where
    F: Fn(&mut Family) -> Result<(T, Vec<DomainEvent>), DomainError>,
{
    let mut family = load_locked_family(conn, family_id, scope).await?;
    let before = Snapshot::capture(&family)?;
    let (value, events) = op(&mut family)?;
    let after = Snapshot::capture(&family)?;
    ensure_change_budget(&before, &after)?;
    apply_changes(conn, &before, &after).await?;
    Ok(Updated { value, events })
}

async fn finish_tx<T>(
    tx: Transaction<'_, Postgres>,
    result: Result<T, TxFailure>,
) -> Result<Result<T, DomainError>, sqlx::Error> {
    match result {
        Ok(value) => {
            tx.commit().await?;
            Ok(Ok(value))
        }
        Err(TxFailure::Domain(err)) => {
            tx.rollback().await?;
            Ok(Err(err))
        }
        Err(TxFailure::Db(err)) => Err(err),
    }
}

async fn load_locked_family(
    conn: &mut PgConnection,
    family_id: FamilyId,
    scope: TokenScope,
) -> Result<Family, TxFailure> {
    let raw_id = family_id.into_inner();
    let family_row = fetch_family_row_for_update(conn, raw_id)
        .await?
        .ok_or(DomainError::FamilyNotFound { family_id })?;
    let member_rows = fetch_member_rows(conn, raw_id).await?;
    let invite_rows = fetch_scoped_token_rows(conn, raw_id, scope).await?;
    Ok(reconstitute_family(family_row, member_rows, invite_rows)?)
}

/// Serializes per-family mutations in one round-trip: `FOR UPDATE` adds the family row to
/// the OCC write set on Aurora DSQL (conflicting writers retry) and locks on PostgreSQL.
async fn fetch_family_row_for_update(
    conn: &mut PgConnection,
    family_id: Uuid,
) -> Result<Option<FamilyRow>, sqlx::Error> {
    sqlx::query_as!(
        FamilyRow,
        "SELECT id, name, home_currency, created_at, deleted_at \
         FROM family_families WHERE id = $1 FOR UPDATE",
        family_id,
    )
    .fetch_optional(conn)
    .await
}

async fn fetch_family_row(
    conn: &mut PgConnection,
    family_id: Uuid,
) -> Result<Option<FamilyRow>, sqlx::Error> {
    sqlx::query_as!(
        FamilyRow,
        "SELECT id, name, home_currency, created_at, deleted_at \
         FROM family_families WHERE id = $1",
        family_id,
    )
    .fetch_optional(conn)
    .await
}

async fn fetch_member_rows(
    conn: &mut PgConnection,
    family_id: Uuid,
) -> Result<Vec<MemberRow>, sqlx::Error> {
    sqlx::query_as!(
        MemberRow,
        "SELECT id, family_id, user_id, display_name, role, permissions, relationship, joined_at, deleted_at \
         FROM family_members WHERE family_id = $1 ORDER BY joined_at ASC, id ASC",
        family_id,
    )
    .fetch_all(conn)
    .await
}

async fn fetch_scoped_token_rows(
    conn: &mut PgConnection,
    family_id: Uuid,
    scope: TokenScope,
) -> Result<Vec<InviteTokenRow>, sqlx::Error> {
    let TokenScope::One(token) = scope else {
        return Ok(Vec::new());
    };
    let row = sqlx::query_as!(
        InviteTokenRow,
        "SELECT token, family_id, role, permissions, relationship, created_by, created_at, expires_at, used \
         FROM family_invite_tokens WHERE family_id = $1 AND token = $2",
        family_id,
        token.into_inner(),
    )
    .fetch_optional(conn)
    .await?;
    Ok(row.into_iter().collect())
}

fn map_member_access(row: MembershipRow) -> Result<MemberAccess, DomainError> {
    Ok(MemberAccess {
        role: parse_role(&row.role)?,
        permissions: parse_permissions(row.permissions)?,
    })
}

fn map_user_family_summary(row: UserFamilyRow) -> Result<UserFamilySummary, DomainError> {
    let name = FamilyName::try_new(row.name)
        .map_err(|err| internal_err("Invalid persisted family name", err))?;
    let home_currency = CurrencyCode::try_new(row.home_currency.trim())
        .map_err(|err| internal_err("Invalid persisted home currency", err))?;
    let role = parse_role(&row.role)?;
    let display_name = MemberDisplayName::try_new(row.display_name)
        .map_err(|err| internal_err("Invalid persisted display name", err))?;

    Ok(UserFamilySummary {
        family: FamilySummary {
            id: FamilyId::from_uuid(row.family_id),
            name,
            home_currency,
        },
        membership: MemberSummary { role, display_name },
    })
}

fn reconstitute_family(
    family_row: FamilyRow,
    member_rows: Vec<MemberRow>,
    invite_rows: Vec<InviteTokenRow>,
) -> Result<Family, DomainError> {
    let name = FamilyName::try_new(family_row.name)
        .map_err(|err| internal_err("Invalid persisted family name", err))?;
    let home_currency = CurrencyCode::try_new(family_row.home_currency.trim())
        .map_err(|err| internal_err("Invalid persisted home currency", err))?;
    let members = member_rows
        .into_iter()
        .map(reconstitute_member)
        .collect::<Result<Vec<_>, _>>()?;
    let invite_tokens = invite_rows
        .into_iter()
        .map(reconstitute_invite_token)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Family::reconstitute(FamilyParams {
        id: FamilyId::from_uuid(family_row.id),
        name,
        home_currency,
        members,
        invite_tokens,
        created_at: family_row.created_at,
        deleted_at: family_row.deleted_at,
    }))
}

fn reconstitute_member(row: MemberRow) -> Result<FamilyMember, DomainError> {
    let display_name = MemberDisplayName::try_new(row.display_name)
        .map_err(|err| internal_err("Invalid persisted member display name", err))?;
    let role = parse_role(&row.role)?;
    let permissions = parse_permissions(row.permissions)?;
    let relationship = parse_optional_relationship(row.relationship)?;

    Ok(FamilyMember::reconstitute(FamilyMemberParams {
        id: MemberId::from_uuid(row.id),
        family_id: FamilyId::from_uuid(row.family_id),
        user_id: UserId::from_uuid(row.user_id),
        display_name,
        role,
        permissions,
        relationship,
        joined_at: row.joined_at,
        deleted_at: row.deleted_at,
    }))
}

fn reconstitute_invite_token(row: InviteTokenRow) -> Result<InviteToken, DomainError> {
    let role = parse_role(&row.role)?;
    let permissions = parse_permissions(row.permissions)?;
    let relationship = parse_optional_relationship(row.relationship)?;

    Ok(InviteToken::reconstitute(InviteTokenParams {
        token: InviteTokenId::from_uuid(row.token),
        family_id: FamilyId::from_uuid(row.family_id),
        role,
        permissions,
        relationship,
        created_by: MemberId::from_uuid(row.created_by),
        created_at: row.created_at,
        expires_at: row.expires_at,
        used: row.used,
    }))
}

fn parse_permissions(value: Value) -> Result<Vec<Permission>, DomainError> {
    serde_json::from_value(value)
        .map_err(|err| internal_err("Invalid persisted permissions JSONB", err))
}

fn parse_role(raw: &str) -> Result<Role, DomainError> {
    Role::try_from(raw).map_err(|err| internal_err("Invalid persisted role", err))
}

fn parse_optional_relationship(raw: Option<String>) -> Result<Option<Relationship>, DomainError> {
    raw.map(Relationship::try_new)
        .transpose()
        .map_err(|err| internal_err("Invalid persisted relationship", err))
}

fn internal_err(context: &'static str, err: impl std::fmt::Display) -> DomainError {
    DomainError::Internal {
        message: format!("{context}: {err}"),
    }
}
