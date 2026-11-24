//! Layer 2a database integration tests executing all 34 migrations against PostgreSQL 16.
//!
//! Run with Docker active:
//! `cargo test -p infrastructure -- --ignored`

use std::collections::BTreeSet;

use anyhow::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use uuid::Uuid;

use infrastructure::db::migrations::run_postgres_migrations;

const EXPECTED_TABLES: [&str; 18] = [
    "family_families",
    "family_members",
    "family_invite_tokens",
    "payment_accounts",
    "account_balance_snapshots",
    "ledger_categories",
    "ledger_transactions",
    "debt_loan_kinds",
    "debt_loans",
    "debt_loan_payments",
    "debt_repayment_plans",
    "recurring_payments",
    "recurring_payment_records",
    "budget_monthly_budgets",
    "budget_envelopes",
    "planning_savings_goals",
    "goal_contributions",
    "exchange_rates",
];

const EXPECTED_INDEXES: [&str; 17] = [
    "idx_members_family",
    "idx_accounts_family",
    "idx_balance_snapshots_account",
    "idx_tx_family_time",
    "idx_tx_account_src",
    "idx_tx_account_dst",
    "idx_loan_kinds_family",
    "idx_loans_family",
    "idx_loan_payments",
    "idx_recurring_family",
    "idx_recurring_due",
    "idx_recurring_linked_loan",
    "idx_recurring_records",
    "idx_goals_family",
    "idx_goal_contributions",
    "idx_exchange_rates",
    "idx_members_user",
];

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn postgres_applies_all_34_migrations_idempotently_and_enforces_constraints() -> Result<()> {
    let (_container, pool) = start_postgres().await?;

    let first_run = run_postgres_migrations(&pool).await?;
    assert_eq!(first_run.total_migrations, 35);
    assert_eq!(first_run.applied_now, 35);
    assert_eq!(first_run.already_applied, 0);
    assert_eq!(first_run.active_async_jobs, 0);

    let second_run = run_postgres_migrations(&pool).await?;
    assert_eq!(second_run.total_migrations, 35);
    assert_eq!(second_run.applied_now, 0);
    assert_eq!(second_run.already_applied, 35);

    let tables = fetch_tables(&pool).await?;
    let expected_tables: BTreeSet<String> =
        EXPECTED_TABLES.into_iter().map(str::to_owned).collect();
    assert_eq!(tables, expected_tables);

    let indexes = fetch_indexes(&pool).await?;
    let expected_indexes: BTreeSet<String> =
        EXPECTED_INDEXES.into_iter().map(str::to_owned).collect();
    assert_eq!(indexes, expected_indexes);

    let (family_id, owner_member_id) = seed_family_with_owner(&pool).await?;
    assert_owner_invite_rejected(&pool, family_id, owner_member_id).await?;
    assert_orphan_member_rejected(&pool).await?;
    Ok(())
}

async fn start_postgres() -> Result<(ContainerAsync<Postgres>, PgPool)> {
    let container = Postgres::default().with_tag("16-alpine").start().await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await?;
    Ok((container, pool))
}

async fn fetch_tables(pool: &PgPool) -> Result<BTreeSet<String>> {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations'",
    )
    .fetch_all(pool)
    .await?;
    Ok(names.into_iter().collect())
}

async fn fetch_indexes(pool: &PgPool) -> Result<BTreeSet<String>> {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT indexname::text FROM pg_indexes \
         WHERE schemaname = 'public' AND indexname LIKE 'idx_%'",
    )
    .fetch_all(pool)
    .await?;
    Ok(names.into_iter().collect())
}

async fn seed_family_with_owner(pool: &PgPool) -> Result<(Uuid, Uuid)> {
    let family_id = Uuid::new_v4();
    let owner_member_id = Uuid::new_v4();

    sqlx::query("INSERT INTO family_families (id, name, home_currency) VALUES ($1, $2, $3)")
        .bind(family_id)
        .bind("Test Family")
        .bind("USD")
        .execute(pool)
        .await?;

    sqlx::query(
        "INSERT INTO family_members (id, family_id, user_id, display_name, role) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(owner_member_id)
    .bind(family_id)
    .bind(Uuid::new_v4())
    .bind("Alice")
    .bind("owner")
    .execute(pool)
    .await?;

    let default_perms: serde_json::Value =
        sqlx::query_scalar("SELECT permissions FROM family_members WHERE id = $1")
            .bind(owner_member_id)
            .fetch_one(pool)
            .await?;
    assert_eq!(default_perms, serde_json::json!([]));
    Ok((family_id, owner_member_id))
}

async fn assert_owner_invite_rejected(
    pool: &PgPool,
    family_id: Uuid,
    owner_member_id: Uuid,
) -> Result<()> {
    let res = sqlx::query(
        "INSERT INTO family_invite_tokens (token, family_id, role, created_by, expires_at) \
         VALUES ($1, $2, 'owner', $3, now() + interval '1 day')",
    )
    .bind(Uuid::new_v4())
    .bind(family_id)
    .bind(owner_member_id)
    .execute(pool)
    .await;
    assert!(
        res.is_err(),
        "invite token with role='owner' must violate CHECK constraint"
    );
    Ok(())
}

async fn assert_orphan_member_rejected(pool: &PgPool) -> Result<()> {
    let res = sqlx::query(
        "INSERT INTO family_members (id, family_id, user_id, display_name, role) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind("Orphan")
    .bind("member")
    .execute(pool)
    .await;
    assert!(
        res.is_err(),
        "unknown family_id must violate FOREIGN KEY constraint"
    );
    Ok(())
}
