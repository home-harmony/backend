//! Targeted write side of the Family aggregate: snapshot, diff, and per-row SQL.
//!
//! Only rows that are new or differ from the snapshot loaded inside the same
//! transaction are written (O(changed rows), never a full-aggregate upsert).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use domain::{
    entities::{Family, FamilyMember, InviteToken},
    errors::DomainError,
};

const MAX_BATCH_ROWS: usize = 500;

/// Failure inside a repository transaction: retryable/db-level or a domain verdict.
pub(super) enum TxFailure {
    Db(sqlx::Error),
    Domain(DomainError),
}

/// Persisted state of an aggregate: header row plus members and loaded tokens by id.
pub(super) struct Snapshot {
    header: PreparedFamily,
    members: BTreeMap<Uuid, PreparedMember>,
    tokens: BTreeMap<Uuid, PreparedInviteToken>,
}

#[derive(PartialEq)]
struct PreparedFamily {
    id: Uuid,
    name: String,
    home_currency: String,
    created_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

#[derive(PartialEq)]
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

#[derive(PartialEq)]
struct PreparedInviteToken {
    id: Uuid,
    token_hash: String,
    family_id: Uuid,
    role: String,
    permissions: Value,
    relationship: Option<String>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    used: bool,
}

impl Snapshot {
    /// Captures the persistable state of `family` (including only the tokens it holds).
    pub(super) fn capture(family: &Family) -> Result<Self, DomainError> {
        let members = family
            .members()
            .iter()
            .map(|m| prepare_member(m).map(|p| (p.id, p)))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let tokens = family
            .invite_tokens()
            .iter()
            .map(|t| prepare_invite_token(t).map(|p| (p.id, p)))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(Self {
            header: prepare_family(family),
            members,
            tokens,
        })
    }

    fn row_count(&self) -> usize {
        1 + self.members.len() + self.tokens.len()
    }
}

impl From<sqlx::Error> for TxFailure {
    fn from(err: sqlx::Error) -> Self {
        Self::Db(err)
    }
}

impl From<DomainError> for TxFailure {
    fn from(err: DomainError) -> Self {
        Self::Domain(err)
    }
}

/// Inserts a brand-new aggregate (plain `INSERT`s, no upsert).
pub(super) async fn insert_aggregate(
    conn: &mut PgConnection,
    snapshot: &Snapshot,
) -> Result<(), TxFailure> {
    if snapshot.row_count() > MAX_BATCH_ROWS {
        return Err(batch_too_large(snapshot.row_count()));
    }
    insert_family_row(conn, &snapshot.header).await?;
    for member in snapshot.members.values() {
        insert_member_row(conn, member).await?;
    }
    for token in snapshot.tokens.values() {
        insert_token_row(conn, token).await?;
    }
    Ok(())
}

/// Rejects a change-set that would exceed the Aurora DSQL batch guard.
pub(super) fn ensure_change_budget(before: &Snapshot, after: &Snapshot) -> Result<(), DomainError> {
    let changed = usize::from(before.header != after.header)
        + after
            .members
            .values()
            .filter(|m| before.members.get(&m.id) != Some(m))
            .count()
        + after
            .tokens
            .values()
            .filter(|t| before.tokens.get(&t.id) != Some(t))
            .count();
    if changed > MAX_BATCH_ROWS {
        return Err(domain_batch_error(changed));
    }
    Ok(())
}

/// Writes only what changed between `before` and `after` (all inside the caller's tx).
pub(super) async fn apply_changes(
    conn: &mut PgConnection,
    before: &Snapshot,
    after: &Snapshot,
) -> Result<(), TxFailure> {
    if before.header != after.header {
        update_family_row(conn, &after.header).await?;
    }
    apply_member_changes(conn, before, after).await?;
    apply_token_changes(conn, before, after).await
}

async fn apply_member_changes(
    conn: &mut PgConnection,
    before: &Snapshot,
    after: &Snapshot,
) -> Result<(), TxFailure> {
    for member in after.members.values() {
        match before.members.get(&member.id) {
            None => insert_member_row(conn, member).await?,
            Some(old) if old != member => update_member_row(conn, member).await?,
            Some(_) => {}
        }
    }
    Ok(())
}

async fn apply_token_changes(
    conn: &mut PgConnection,
    before: &Snapshot,
    after: &Snapshot,
) -> Result<(), TxFailure> {
    for token in after.tokens.values() {
        match before.tokens.get(&token.id) {
            None => insert_token_row(conn, token).await?,
            Some(old) if old != token => consume_token_row(conn, token).await?,
            Some(_) => {}
        }
    }
    Ok(())
}

fn batch_too_large(rows: usize) -> TxFailure {
    TxFailure::Domain(domain_batch_error(rows))
}

fn domain_batch_error(rows: usize) -> DomainError {
    DomainError::InvariantViolation {
        message: format!(
            "Family write batch exceeds Aurora DSQL limit ({rows} > {MAX_BATCH_ROWS})"
        ),
    }
}

fn prepare_family(family: &Family) -> PreparedFamily {
    PreparedFamily {
        id: family.id().into_inner(),
        name: family.name().as_str().to_owned(),
        home_currency: family.home_currency().as_str().to_owned(),
        created_at: family.created_at(),
        deleted_at: family.deleted_at(),
    }
}

fn prepare_member(member: &FamilyMember) -> Result<PreparedMember, DomainError> {
    let permissions =
        serde_json::to_value(member.permissions()).map_err(|err| DomainError::Internal {
            message: format!("Failed to serialize member permissions: {err}"),
        })?;
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
    let permissions =
        serde_json::to_value(invite.permissions()).map_err(|err| DomainError::Internal {
            message: format!("Failed to serialize invite permissions: {err}"),
        })?;
    Ok(PreparedInviteToken {
        id: invite.id().into_inner(),
        token_hash: invite.token_hash().as_str().to_owned(),
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

async fn insert_family_row(
    conn: &mut PgConnection,
    family: &PreparedFamily,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO family_families (id, name, home_currency, created_at, deleted_at) \
         VALUES ($1, $2, $3, $4, $5)",
        family.id,
        family.name,
        family.home_currency,
        family.created_at,
        family.deleted_at,
    )
    .execute(conn)
    .await
    .map(|_| ())
}

async fn update_family_row(
    conn: &mut PgConnection,
    family: &PreparedFamily,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE family_families \
         SET name = $2, home_currency = $3, deleted_at = $4 \
         WHERE id = $1",
        family.id,
        family.name,
        family.home_currency,
        family.deleted_at,
    )
    .execute(conn)
    .await
    .map(|_| ())
}

async fn insert_member_row(
    conn: &mut PgConnection,
    member: &PreparedMember,
) -> Result<(), TxFailure> {
    let result = sqlx::query!(
        "INSERT INTO family_members \
             (id, family_id, user_id, display_name, role, permissions, relationship, joined_at, deleted_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
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
    .await;
    match result {
        Ok(_) => Ok(()),
        Err(err) => Err(map_member_insert_error(err, member.user_id)),
    }
}

/// Maps the `(family_id, user_id)` unique violation (SQLSTATE `23505`) to a domain error.
fn map_member_insert_error(err: sqlx::Error, user_id: Uuid) -> TxFailure {
    let is_unique = err
        .as_database_error()
        .is_some_and(|db| db.is_unique_violation());
    if is_unique {
        return TxFailure::Domain(DomainError::MemberAlreadyExists {
            user_id: domain::value_objects::UserId::from_uuid(user_id),
        });
    }
    TxFailure::Db(err)
}

async fn update_member_row(
    conn: &mut PgConnection,
    member: &PreparedMember,
) -> Result<(), TxFailure> {
    let result = sqlx::query!(
        "UPDATE family_members \
         SET display_name = $3, role = $4, permissions = $5, relationship = $6, \
             joined_at = $7, deleted_at = $8 \
         WHERE id = $1 AND family_id = $2",
        member.id,
        member.family_id,
        member.display_name,
        member.role,
        member.permissions,
        member.relationship,
        member.joined_at,
        member.deleted_at,
    )
    .execute(conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(TxFailure::Domain(DomainError::Internal {
            message: "Member row to update was not found in its family".to_owned(),
        }));
    }
    Ok(())
}

async fn insert_token_row(
    conn: &mut PgConnection,
    invite: &PreparedInviteToken,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO family_invite_tokens \
             (id, token_hash, family_id, role, permissions, relationship, created_by, created_at, expires_at, used) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        invite.id,
        invite.token_hash,
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

/// Conditional single-use transition: succeeds only if the token is still unused.
async fn consume_token_row(
    conn: &mut PgConnection,
    invite: &PreparedInviteToken,
) -> Result<(), TxFailure> {
    let result = sqlx::query!(
        "UPDATE family_invite_tokens SET used = $3 \
         WHERE id = $1 AND family_id = $2 AND used = false",
        invite.id,
        invite.family_id,
        invite.used,
    )
    .execute(conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(TxFailure::Domain(DomainError::TokenAlreadyUsed));
    }
    Ok(())
}
