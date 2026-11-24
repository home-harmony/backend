//! Offline verification of Aurora DSQL migration invariants across `backend/migrations/`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use infrastructure::db::migrations::{dsql_migrator, postgres_migrator, validate_dsql_invariants};

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

const EXPECTED_INDEXES: [&str; 18] = [
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
    "idx_invite_tokens_family",
];

const BANNED_SQL_TOKENS: [&str; 7] = [
    "SERIAL",
    "BIGSERIAL",
    "TRUNCATE",
    "CREATE TRIGGER",
    "INSERT INTO",
    "UPDATE ",
    "DELETE FROM",
];

#[test]
fn embedded_dsql_migrator_has_ordered_non_transactional_migrations() -> Result<()> {
    let migrator = dsql_migrator();
    assert!(!migrator.locking);
    validate_dsql_invariants(&migrator)?;

    let versions: Vec<i64> = migrator.iter().map(|m| m.version).collect();
    let mut expected: Vec<i64> = (20260616000001..=20260616000034).collect();
    expected.push(20261124000001);
    expected.push(20261124000002);
    assert_eq!(versions, expected);
    Ok(())
}

#[test]
fn disk_migration_files_enforce_single_ddl_and_dsql_constraints() -> Result<()> {
    let files = collect_migration_files()?;
    assert_eq!(files.len(), 36);

    for path in &files {
        assert_file_satisfies_dsql_rules(path)?;
    }
    Ok(())
}

#[test]
fn table_migrations_match_schema_catalog_and_invite_check() {
    let migrator = dsql_migrator();
    let migrations: Vec<_> = migrator.iter().collect();
    assert_eq!(migrations.len(), 36);

    for (idx, table) in EXPECTED_TABLES.iter().enumerate() {
        let sql = migrations[idx].sql.as_str();
        assert!(
            sql.contains(&format!("CREATE TABLE IF NOT EXISTS {table} ")),
            "migration {} missing expected table `{table}`",
            migrations[idx].version
        );
    }

    let invite_sql = migrations[2].sql.as_str();
    assert!(
        invite_sql.contains("CHECK (role IN ('member','child','other'))"),
        "family_invite_tokens must reject 'owner' invites"
    );
}

#[test]
fn index_migrations_use_async_and_rewrite_cleanly_for_postgres() {
    let migrator = dsql_migrator();
    let migrations: Vec<_> = migrator.iter().collect();
    assert_eq!(migrations.len(), 36);

    for (offset, index_name) in EXPECTED_INDEXES.iter().enumerate() {
        let sql = migrations[18 + offset].sql.as_str();
        let upper = sql.to_ascii_uppercase();
        assert!(
            sql.contains(&format!("CREATE INDEX ASYNC IF NOT EXISTS {index_name} ")),
            "migration {} missing expected async index `{index_name}`",
            migrations[18 + offset].version
        );
        assert!(
            !contains_token(&upper, "DESC") && !contains_token(&upper, "ASC"),
            "Aurora DSQL does not support ASC/DESC in index definitions ({index_name})"
        );
    }

    let pg_migrator = postgres_migrator();
    assert_eq!(
        pg_migrator
            .iter()
            .map(|m| m.version)
            .collect::<BTreeSet<_>>()
            .len(),
        36
    );
    for migration in pg_migrator.iter() {
        assert!(migration.no_tx && !migration.sql.as_str().contains("CREATE INDEX ASYNC"));
    }
}

fn assert_file_satisfies_dsql_rules(path: &Path) -> Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let first_line = content.lines().next().unwrap_or("");
    if first_line.trim() != "-- no-transaction" {
        bail!(
            "missing `-- no-transaction` on line 1 of {}",
            path.display()
        );
    }

    let statements = split_ddl_statements(&content);
    if statements.len() != 1 {
        bail!("expected exactly 1 DDL statement in {}", path.display());
    }

    let upper = statements[0].to_ascii_uppercase();
    for banned in BANNED_SQL_TOKENS {
        if contains_token(&upper, banned) {
            bail!("banned token `{banned}` found in {}", path.display());
        }
    }
    Ok(())
}

fn collect_migration_files() -> Result<Vec<PathBuf>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("infrastructure crate must have backend parent directory")?
        .join("migrations");

    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("sql") {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        bail!("no .sql files found in {}", dir.display());
    }
    Ok(files)
}

fn split_ddl_statements(sql: &str) -> Vec<String> {
    let stripped: String = sql
        .lines()
        .map(strip_line_comment)
        .collect::<Vec<_>>()
        .join("\n");

    stripped
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn strip_line_comment(line: &str) -> &str {
    match line.split_once("--") {
        Some((before, _)) => before,
        None => line,
    }
}

fn contains_token(haystack_upper: &str, token: &str) -> bool {
    if token.contains(' ') || token.ends_with(' ') {
        return haystack_upper.contains(token);
    }
    haystack_upper
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|word| word == token)
}
