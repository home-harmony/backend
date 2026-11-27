//! Idempotent post-migration database seeding and role-constraint reconciliation.
//!
//! Aurora DSQL DDL migrations (`backend/migrations/*.sql`) strictly disallow DML
//! statements (`INSERT`, `UPDATE`, `DELETE`). Therefore, global system categories
//! (`family_id IS NULL`) and legacy `'other' -> 'guest'` row normalization execute
//! here inside `retry_on_occ` transactions from `migrate_runner` (and PG16 test setups).

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use aurora_dsql_sqlx_connector::{OCCRetryConfig, retry_on_occ};
use sqlx::PgPool;
use uuid::Uuid;

use domain::value_objects::{CategoryIcon, CategoryKind, CategoryName, HexColor};

/// Compile-time definition of a read-only global system category (`family_id IS NULL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemCategorySeed {
    pub id: Uuid,
    pub name: &'static str,
    pub kind: CategoryKind,
    pub icon: &'static str,
    pub color: &'static str,
}

/// Curated system default categories shared across all family workspaces.
pub const SYSTEM_CATEGORIES: &[SystemCategorySeed] = &[
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb01),
        name: "Salary",
        kind: CategoryKind::Income,
        icon: "briefcase",
        color: "#15803D",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb02),
        name: "Bonus & Gifts",
        kind: CategoryKind::Income,
        icon: "gift",
        color: "#16A34A",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb03),
        name: "Interest & Dividends",
        kind: CategoryKind::Income,
        icon: "trending-up",
        color: "#0D9488",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb04),
        name: "Other Income",
        kind: CategoryKind::Income,
        icon: "plus-circle",
        color: "#059669",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb05),
        name: "Groceries",
        kind: CategoryKind::Expense,
        icon: "shopping-cart",
        color: "#EA580C",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb06),
        name: "Housing & Rent",
        kind: CategoryKind::Expense,
        icon: "home",
        color: "#2563EB",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb07),
        name: "Utilities",
        kind: CategoryKind::Expense,
        icon: "zap",
        color: "#CA8A04",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb08),
        name: "Dining Out",
        kind: CategoryKind::Expense,
        icon: "utensils",
        color: "#E11D48",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb09),
        name: "Transportation",
        kind: CategoryKind::Expense,
        icon: "car",
        color: "#4F46E5",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb0a),
        name: "Healthcare",
        kind: CategoryKind::Expense,
        icon: "heart-pulse",
        color: "#DC2626",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb0b),
        name: "Education",
        kind: CategoryKind::Expense,
        icon: "graduation-cap",
        color: "#7C3AED",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb0c),
        name: "Entertainment",
        kind: CategoryKind::Expense,
        icon: "film",
        color: "#9333EA",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb0d),
        name: "Shopping",
        kind: CategoryKind::Expense,
        icon: "shopping-bag",
        color: "#DB2777",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb0e),
        name: "Subscriptions",
        kind: CategoryKind::Expense,
        icon: "repeat",
        color: "#0284C7",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb0f),
        name: "Account Transfer",
        kind: CategoryKind::Transfer,
        icon: "arrow-left-right",
        color: "#475569",
    },
    SystemCategorySeed {
        id: Uuid::from_u128(0x9b1deb4d_3b7d_4bad_9bdd_2b0d7b3dcb10),
        name: "ATM Cash Withdrawal",
        kind: CategoryKind::Transfer,
        icon: "banknote",
        color: "#0F766E",
    },
];

/// Rewrites any legacy `'other'` role rows to `'guest'` under OCC retry before constraint validation.
pub async fn normalize_legacy_other_roles(pool: &PgPool) -> Result<u64> {
    let occ = OCCRetryConfig::default();
    retry_on_occ(&occ, || async {
        let mut tx = pool.begin().await?;
        let members =
            sqlx::query("UPDATE family_members SET role = 'guest' WHERE role = 'other'")
                .execute(&mut *tx)
                .await?
                .rows_affected();
        let invites =
            sqlx::query("UPDATE family_invite_tokens SET role = 'guest' WHERE role = 'other'")
                .execute(&mut *tx)
                .await?
                .rows_affected();
        tx.commit().await?;
        Ok::<u64, sqlx::Error>(members + invites)
    })
    .await
    .context("Failed to normalize legacy 'other' roles to 'guest'")
}

/// Validates `CHECK (role IN (... 'guest'))` constraints added with `NOT VALID`.
///
/// Set `use_async = true` on Aurora DSQL (`ALTER TABLE ASYNC ... VALIDATE CONSTRAINT`)
/// and `false` on standard PostgreSQL 16.
pub async fn validate_role_check_constraints(pool: &PgPool, use_async: bool) -> Result<()> {
    let prefix = if use_async {
        "ALTER TABLE ASYNC"
    } else {
        "ALTER TABLE"
    };
    let members_sql = format!(
        "{prefix} family_members VALIDATE CONSTRAINT family_members_role_check"
    );
    let invites_sql = format!(
        "{prefix} family_invite_tokens VALIDATE CONSTRAINT family_invite_tokens_role_check"
    );
    let occ = OCCRetryConfig::default();
    retry_on_occ(&occ, || async {
        sqlx::query(&members_sql).execute(pool).await?;
        sqlx::query(&invites_sql).execute(pool).await?;
        Ok::<(), sqlx::Error>(())
    })
    .await
    .context("Failed to validate role CHECK constraints")
}

/// Idempotently inserts all global system default categories (`family_id IS NULL`).
pub async fn seed_system_categories(pool: &PgPool) -> Result<u64> {
    let occ = OCCRetryConfig::default();
    retry_on_occ(&occ, || async {
        let mut tx = pool.begin().await?;
        let mut inserted = 0u64;
        for cat in SYSTEM_CATEGORIES {
            let res = sqlx::query(
                "INSERT INTO ledger_categories (id, family_id, name, kind, icon, color) \
                 VALUES ($1, NULL, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING",
            )
            .bind(cat.id)
            .bind(cat.name)
            .bind(cat.kind.as_str())
            .bind(cat.icon)
            .bind(cat.color)
            .execute(&mut *tx)
            .await?;
            inserted += res.rows_affected();
        }
        tx.commit().await?;
        Ok::<u64, sqlx::Error>(inserted)
    })
    .await
    .context("Failed to seed system default ledger_categories")
}

/// Validates static invariants of [`SYSTEM_CATEGORIES`] (unique v4 UUIDs and valid domain VOs).
pub fn validate_system_catalog() -> Result<()> {
    let mut ids = BTreeSet::new();
    for cat in SYSTEM_CATEGORIES {
        anyhow::ensure!(
            cat.id.get_version_num() == 4,
            "System category {} must use UUID v4",
            cat.name
        );
        anyhow::ensure!(
            ids.insert(cat.id),
            "Duplicate system category UUID: {}",
            cat.id
        );
        CategoryName::try_new(cat.name)?;
        CategoryIcon::try_new(cat.icon)?;
        HexColor::try_new(cat.color)?;
    }
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_category_catalog_has_valid_unique_v4_entries() {
        validate_system_catalog().unwrap();
        assert!(
            SYSTEM_CATEGORIES
                .iter()
                .any(|c| c.kind == CategoryKind::Income)
        );
        assert!(
            SYSTEM_CATEGORIES
                .iter()
                .any(|c| c.kind == CategoryKind::Expense)
        );
        assert!(
            SYSTEM_CATEGORIES
                .iter()
                .any(|c| c.kind == CategoryKind::Transfer)
        );
    }
}
