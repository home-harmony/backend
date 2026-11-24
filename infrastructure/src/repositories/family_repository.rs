//! Aurora DSQL persistence adapter for [`FamilyRepository`] and [`MembershipReader`].

use aurora_dsql_sqlx_connector::{OCCRetryConfig, retry_on_occ};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use domain::{
    entities::{
        Family, FamilyMember, FamilyMemberParams, FamilyParams, InviteToken, InviteTokenParams,
    },
    errors::DomainError,
    ports::{FamilyRepository, MemberAccess, MembershipReader, UserFamilySummary},
    value_objects::{
        CurrencyCode, DisplayName, FamilyId, FamilyName, InviteTokenId, MemberId, Permission,
        Relationship, Role, UserId,
    },
};

const MAX_BATCH_ROWS: usize = 500;

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

struct PreparedFamilySave {
    id: Uuid,
    name: String,
    home_currency: String,
    created_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    members: Vec<PreparedMember>,
    invite_tokens: Vec<PreparedInviteToken>,
}

struct PreparedMember {
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

struct PreparedInviteToken {
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
        let Some(family_row) = fetch_family_row(&mut tx, raw_id).await? else {
            tx.commit()
                .await
                .map_err(|err| internal_err("Failed to commit read-only family tx", err))?;
            return Ok(None);
        };
        let member_rows = fetch_member_rows(&mut tx, raw_id).await?;
        let invite_rows = fetch_invite_token_rows(&mut tx, raw_id).await?;

        tx.commit()
            .await
            .map_err(|err| internal_err("Failed to commit read-only family tx", err))?;

        reconstitute_family(family_row, member_rows, invite_rows).map(Some)
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

    async fn save(&self, family: &Family) -> Result<(), DomainError> {
        let prepared = prepare_family_save(family)?;
        execute_family_save(&self.pool, &self.occ_config, &prepared).await
    }
}

async fn fetch_family_row(
    conn: &mut PgConnection,
    family_id: Uuid,
) -> Result<Option<FamilyRow>, DomainError> {
    sqlx::query_as!(
        FamilyRow,
        "SELECT id, name, home_currency, created_at, deleted_at \
         FROM family_families WHERE id = $1",
        family_id,
    )
    .fetch_optional(conn)
    .await
    .map_err(|err| internal_err("Failed to query family_families row", err))
}

async fn fetch_member_rows(
    conn: &mut PgConnection,
    family_id: Uuid,
) -> Result<Vec<MemberRow>, DomainError> {
    sqlx::query_as!(
        MemberRow,
        "SELECT id, family_id, user_id, display_name, role, permissions, relationship, joined_at, deleted_at \
         FROM family_members WHERE family_id = $1 ORDER BY joined_at ASC, id ASC",
        family_id,
    )
    .fetch_all(conn)
    .await
    .map_err(|err| internal_err("Failed to query family_members rows", err))
}

async fn fetch_invite_token_rows(
    conn: &mut PgConnection,
    family_id: Uuid,
) -> Result<Vec<InviteTokenRow>, DomainError> {
    sqlx::query_as!(
        InviteTokenRow,
        "SELECT token, family_id, role, permissions, relationship, created_by, created_at, expires_at, used \
         FROM family_invite_tokens WHERE family_id = $1 ORDER BY created_at ASC, token ASC",
        family_id,
    )
    .fetch_all(conn)
    .await
    .map_err(|err| internal_err("Failed to query family_invite_tokens rows", err))
}

fn prepare_family_save(family: &Family) -> Result<PreparedFamilySave, DomainError> {
    let total_rows = 1usize
        .saturating_add(family.members().len())
        .saturating_add(family.invite_tokens().len());
    if total_rows > MAX_BATCH_ROWS {
        return Err(DomainError::InvariantViolation {
            message: format!(
                "Family write batch exceeds Aurora DSQL limit ({total_rows} > {MAX_BATCH_ROWS})"
            ),
        });
    }
    let members = family
        .members()
        .iter()
        .map(prepare_member)
        .collect::<Result<Vec<_>, _>>()?;
    let invite_tokens = family
        .invite_tokens()
        .iter()
        .map(prepare_invite_token)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PreparedFamilySave {
        id: family.id().into_inner(),
        name: family.name().as_str().to_owned(),
        home_currency: family.home_currency().as_str().to_owned(),
        created_at: family.created_at(),
        deleted_at: family.deleted_at(),
        members,
        invite_tokens,
    })
}

fn prepare_member(member: &FamilyMember) -> Result<PreparedMember, DomainError> {
    let permissions = serde_json::to_value(member.permissions())
        .map_err(|err| internal_err("Failed to serialize member permissions", err))?;
    Ok(PreparedMember {
        id: member.id().into_inner(),
        family_id: member.family_id().into_inner(),
        user_id: member.user_id().into_inner(),
        display_name: member.display_name().as_str().to_owned(),
        role: member.role().to_string(),
        permissions,
        relationship: member.relationship().map(|rel| rel.as_str().to_owned()),
        joined_at: member.joined_at(),
        deleted_at: member.deleted_at(),
    })
}

fn prepare_invite_token(invite: &InviteToken) -> Result<PreparedInviteToken, DomainError> {
    let permissions = serde_json::to_value(invite.permissions())
        .map_err(|err| internal_err("Failed to serialize invite permissions", err))?;
    Ok(PreparedInviteToken {
        token: invite.token().into_inner(),
        family_id: invite.family_id().into_inner(),
        role: invite.role().to_string(),
        permissions,
        relationship: invite.relationship().map(|rel| rel.as_str().to_owned()),
        created_by: invite.created_by().into_inner(),
        created_at: invite.created_at(),
        expires_at: invite.expires_at(),
        used: invite.is_used(),
    })
}

async fn execute_family_save(
    pool: &PgPool,
    occ_config: &OCCRetryConfig,
    payload: &PreparedFamilySave,
) -> Result<(), DomainError> {
    retry_on_occ(occ_config, || async {
        let mut tx = pool.begin().await?;
        upsert_family_row(&mut tx, payload).await?;
        for member in &payload.members {
            upsert_member_row(&mut tx, member).await?;
        }
        for invite in &payload.invite_tokens {
            upsert_invite_token_row(&mut tx, invite).await?;
        }
        tx.commit().await
    })
    .await
    .map_err(|err| internal_err("Failed to persist Family aggregate", err))
}

async fn upsert_family_row(
    conn: &mut PgConnection,
    payload: &PreparedFamilySave,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO family_families (id, name, home_currency, created_at, deleted_at) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (id) DO UPDATE SET \
             name = EXCLUDED.name, \
             home_currency = EXCLUDED.home_currency, \
             deleted_at = EXCLUDED.deleted_at",
        payload.id,
        payload.name,
        payload.home_currency,
        payload.created_at,
        payload.deleted_at,
    )
    .execute(conn)
    .await
    .map(|_| ())
}

async fn upsert_member_row(
    conn: &mut PgConnection,
    member: &PreparedMember,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO family_members \
             (id, family_id, user_id, display_name, role, permissions, relationship, joined_at, deleted_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (id) DO UPDATE SET \
             display_name = EXCLUDED.display_name, \
             role = EXCLUDED.role, \
             permissions = EXCLUDED.permissions, \
             relationship = EXCLUDED.relationship, \
             joined_at = EXCLUDED.joined_at, \
             deleted_at = EXCLUDED.deleted_at",
        member.id,
        member.family_id,
        member.user_id,
        member.display_name,
        member.role,
        member.permissions,
        member.relationship,
        member.joined_at,
        member.deleted_at,
    )
    .execute(conn)
    .await
    .map(|_| ())
}

async fn upsert_invite_token_row(
    conn: &mut PgConnection,
    invite: &PreparedInviteToken,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO family_invite_tokens \
             (token, family_id, role, permissions, relationship, created_by, created_at, expires_at, used) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (token) DO UPDATE SET \
             used = EXCLUDED.used",
        invite.token,
        invite.family_id,
        invite.role,
        invite.permissions,
        invite.relationship,
        invite.created_by,
        invite.created_at,
        invite.expires_at,
        invite.used,
    )
    .execute(conn)
    .await
    .map(|_| ())
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
    let display_name = DisplayName::try_new(row.display_name)
        .map_err(|err| internal_err("Invalid persisted display name", err))?;

    Ok(UserFamilySummary {
        family_id: FamilyId::from_uuid(row.family_id),
        name,
        home_currency,
        role,
        display_name,
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
    let display_name = DisplayName::try_new(row.display_name)
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
