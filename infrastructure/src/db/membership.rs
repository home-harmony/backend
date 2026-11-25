//! Read-only family membership verification adapter (`BEGIN READ ONLY`).

use sqlx::{PgPool, Row};

use domain::{
    ports::{ActiveFamilyMembership, FamilyMembershipReader},
    value_objects::{FamilyId, MemberId, Permission, Role, UserId},
};

/// Errors returned while querying active family membership in Aurora DSQL / PostgreSQL.
#[derive(Debug, thiserror::Error)]
pub enum MembershipLookupError {
    #[error("Database query failed while resolving family membership: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Stored member role or permissions failed domain validation: {0}")]
    InvalidPersistedData(String),
}

/// SQLx-backed implementation of [`FamilyMembershipReader`].
#[derive(Debug, Clone)]
pub struct PgFamilyMembershipReader {
    pool: PgPool,
}

impl PgFamilyMembershipReader {
    /// Creates a membership reader backed by `pool`.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl FamilyMembershipReader for PgFamilyMembershipReader {
    type Error = MembershipLookupError;

    async fn find_active_membership(
        &self,
        family_id: FamilyId,
        user_id: UserId,
    ) -> Result<Option<ActiveFamilyMembership>, Self::Error> {
        let mut tx = self.pool.begin_with("BEGIN READ ONLY").await?;
        let row = sqlx::query(
            "SELECT id, role, permissions \
             FROM family_members \
             WHERE family_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(family_id.into_inner())
        .bind(user_id.into_inner())
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;

        let Some(row) = row else {
            return Ok(None);
        };

        parse_membership_row(&row, family_id, user_id).map(Some)
    }
}

fn parse_membership_row(
    row: &sqlx::postgres::PgRow,
    family_id: FamilyId,
    user_id: UserId,
) -> Result<ActiveFamilyMembership, MembershipLookupError> {
    let member_uuid: uuid::Uuid = row.try_get("id")?;
    let role_str: String = row.try_get("role")?;
    let perms_json: sqlx::types::Json<Vec<Permission>> = row.try_get("permissions")?;
    let role = Role::try_from(role_str.as_str())
        .map_err(|e| MembershipLookupError::InvalidPersistedData(e.to_string()))?;

    Ok(ActiveFamilyMembership {
        member_id: MemberId::from_uuid(member_uuid),
        family_id,
        user_id,
        role,
        permissions: perms_json.0,
    })
}
