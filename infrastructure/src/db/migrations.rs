//! Shared SQLx migration engine for Amazon Aurora DSQL and PostgreSQL 16 tests.
//!
//! Aurora DSQL imposes constraints that differ from standard PostgreSQL:
//! - Advisory locks (`pg_advisory_lock`) are unsupported (`Migrator::set_locking(false)`).
//! - DDL and `_sqlx_migrations` DML cannot share a transaction (`-- no-transaction`).
//! - Catalog updates after `CREATE INDEX ASYNC` can raise OCC conflicts (`SQLSTATE 40001` / `OC001`).
//! - Index creation uses `CREATE INDEX ASYNC`, which must be rewritten to `CREATE INDEX`
//!   when running integration tests against standard PostgreSQL 16.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use aurora_dsql_sqlx_connector::is_occ_error;
use serde::Serialize;
use sqlx::migrate::{MigrateError, Migration, Migrator};
use sqlx::{AssertSqlSafe, PgPool, Row, SqlSafeStr};
use tracing::{info, warn};

const DEFAULT_MAX_OCC_ATTEMPTS: u32 = 5;
const OCC_BASE_DELAY_MS: u64 = 50;
const OCC_MAX_DELAY_MS: u64 = 1_000;

/// Summary returned after a migration pass completes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationReport {
    pub total_migrations: usize,
    pub applied_now: usize,
    pub already_applied: usize,
    pub active_async_jobs: usize,
}

/// Execution options for [`run_dsql_migrations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsqlMigrationOptions {
    /// Whether to wait for background `CREATE INDEX ASYNC` jobs via `sys.wait_for_job`.
    pub wait_for_indexes: bool,
    /// Maximum OCC retry attempts when Aurora DSQL raises `40001`/`OC000`/`OC001`.
    pub max_occ_attempts: u32,
}

impl Default for DsqlMigrationOptions {
    fn default() -> Self {
        Self {
            wait_for_indexes: true,
            max_occ_attempts: DEFAULT_MAX_OCC_ATTEMPTS,
        }
    }
}

/// Returns the embedded Aurora DSQL migrator with advisory locking disabled.
#[must_use]
pub fn dsql_migrator() -> Migrator {
    let mut migrator = embedded_raw_migrator();
    // Aurora DSQL does not support pg_advisory_lock; deployments must be serialized externally.
    migrator.set_locking(false);
    migrator
}

/// Returns a PostgreSQL 16-compatible migrator derived from the same embedded SQL files.
#[must_use]
pub fn postgres_migrator() -> Migrator {
    let raw = embedded_raw_migrator();
    let rewritten: Vec<Migration> = raw.iter().map(to_postgres_migration).collect();
    let mut migrator = Migrator::with_migrations(rewritten);
    migrator.set_locking(false);
    migrator
}

/// Validates that all embedded migrations satisfy Aurora DSQL runner invariants.
///
/// # Errors
///
/// Returns an error if no migrations are embedded or any file lacks `-- no-transaction`.
pub fn validate_dsql_invariants(migrator: &Migrator) -> Result<()> {
    if migrator.iter().len() == 0 {
        bail!("No embedded SQL migrations found");
    }

    for migration in migrator.iter() {
        if !migration.no_tx {
            bail!(
                "Migration {} ({}) must start with `-- no-transaction` for Aurora DSQL",
                migration.version,
                migration.description
            );
        }
    }

    Ok(())
}

/// Strips Aurora DSQL's `ASYNC` index keyword so tests can run against PostgreSQL 16.
#[must_use]
pub fn rewrite_sql_for_postgres(sql: &str) -> String {
    sql.replace("CREATE UNIQUE INDEX ASYNC", "CREATE UNIQUE INDEX")
        .replace("CREATE INDEX ASYNC", "CREATE INDEX")
}

/// Applies all pending embedded migrations against an Aurora DSQL pool.
///
/// # Errors
///
/// Returns an error if invariants fail, OCC retries are exhausted, a migration fails,
/// or any asynchronous index build fails in `sys.jobs`.
pub async fn run_dsql_migrations(
    pool: &PgPool,
    options: DsqlMigrationOptions,
) -> Result<MigrationReport> {
    let migrator = dsql_migrator();
    validate_dsql_invariants(&migrator)?;

    let total_migrations = migrator.iter().len();
    let already_applied = count_applied_migrations(pool).await?;

    run_migrator_with_occ_retry(&migrator, pool, options.max_occ_attempts).await?;

    let after_applied = count_applied_migrations(pool).await?;
    let active_async_jobs = reconcile_async_index_jobs(pool, options.wait_for_indexes).await?;

    Ok(MigrationReport {
        total_migrations,
        applied_now: after_applied.saturating_sub(already_applied),
        already_applied,
        active_async_jobs,
    })
}

/// Applies all pending embedded migrations against a standard PostgreSQL 16 pool.
///
/// # Errors
///
/// Returns an error if executing migrations or reading `_sqlx_migrations` fails.
pub async fn run_postgres_migrations(pool: &PgPool) -> Result<MigrationReport> {
    let migrator = postgres_migrator();
    let total_migrations = migrator.iter().len();
    let already_applied = count_applied_migrations(pool).await?;

    migrator
        .run(pool)
        .await
        .context("PostgreSQL migration run failed")?;

    let after_applied = count_applied_migrations(pool).await?;

    Ok(MigrationReport {
        total_migrations,
        applied_now: after_applied.saturating_sub(already_applied),
        already_applied,
        active_async_jobs: 0,
    })
}

fn embedded_raw_migrator() -> Migrator {
    sqlx::migrate!("../migrations")
}

fn to_postgres_migration(migration: &Migration) -> Migration {
    let sql = rewrite_sql_for_postgres(migration.sql.as_str());
    Migration::new(
        migration.version,
        migration.description.clone(),
        migration.migration_type,
        AssertSqlSafe(sql).into_sql_str(),
        migration.no_tx,
    )
}

async fn count_applied_migrations(pool: &PgPool) -> Result<usize> {
    let mut tx = pool.begin().await.context("Failed to start read tx")?;
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .context("Failed to mark _sqlx_migrations read tx READ ONLY")?;

    let result: std::result::Result<i64, sqlx::Error> =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = true")
            .fetch_one(&mut *tx)
            .await;

    match result {
        Ok(count) => {
            tx.commit().await.context("Failed to commit read tx")?;
            usize::try_from(count).context("Negative migration count in _sqlx_migrations")
        }
        Err(err) if is_undefined_table_error(&err) => Ok(0),
        Err(err) => Err(err).context("Failed to query _sqlx_migrations"),
    }
}

fn is_undefined_table_error(err: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db_err) = err else {
        return false;
    };
    db_err.code().as_deref() == Some("42P01")
}

async fn run_migrator_with_occ_retry(
    migrator: &Migrator,
    pool: &PgPool,
    max_attempts: u32,
) -> Result<()> {
    let max_attempts = max_attempts.max(1);
    let mut attempt = 1;

    loop {
        let Err(err) = migrator.run(pool).await else {
            return Ok(());
        };
        let Some(occ_code) = extract_occ_code(&err) else {
            return Err(err).context("Migration run failed");
        };
        if attempt >= max_attempts {
            return Err(err).context(format!(
                "Migration run exhausted {max_attempts} attempts on OCC conflict ({occ_code})"
            ));
        }

        let delay = occ_backoff_delay(attempt);
        warn!(
            attempt,
            max_attempts,
            occ_code,
            delay_ms = delay.as_millis(),
            "OCC conflict during migration run; retrying"
        );
        tokio::time::sleep(delay).await;
        attempt += 1;
    }
}

fn extract_occ_code(err: &MigrateError) -> Option<&'static str> {
    let sqlx_err = match err {
        MigrateError::Execute(inner) | MigrateError::ExecuteMigration(inner, _) => inner,
        _ => return None,
    };
    is_occ_error(sqlx_err).map(|(_, code)| code)
}

fn occ_backoff_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(10);
    let delay_ms = OCC_BASE_DELAY_MS.saturating_mul(1u64 << shift);
    Duration::from_millis(delay_ms.min(OCC_MAX_DELAY_MS))
}

async fn reconcile_async_index_jobs(pool: &PgPool, wait_for_indexes: bool) -> Result<usize> {
    let (active, failed) = query_pending_async_jobs(pool).await?;
    if !failed.is_empty() {
        bail!(
            "Aurora DSQL async index job(s) failed: {}",
            failed.join(", ")
        );
    }
    if !wait_for_indexes || active.is_empty() {
        return Ok(active.len());
    }

    info!(
        pending_jobs = active.len(),
        "Waiting for Aurora DSQL async index jobs to complete"
    );
    for job_id in &active {
        wait_for_dsql_job(pool, job_id).await?;
    }

    let (remaining_active, remaining_failed) = query_pending_async_jobs(pool).await?;
    if !remaining_failed.is_empty() {
        bail!(
            "Aurora DSQL async index job(s) failed after wait: {}",
            remaining_failed.join(", ")
        );
    }
    Ok(remaining_active.len())
}

async fn query_pending_async_jobs(pool: &PgPool) -> Result<(Vec<String>, Vec<String>)> {
    let mut tx = pool.begin().await.context("Failed to start sys.jobs tx")?;
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .context("Failed to mark sys.jobs tx READ ONLY")?;

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT job_id::text, LOWER(status) FROM sys.jobs \
         WHERE LOWER(status) IN ('submitted', 'processing', 'running', 'failed')",
    )
    .fetch_all(&mut *tx)
    .await
    .context("Failed to inspect Aurora DSQL sys.jobs view")?;
    tx.commit().await.context("Failed to commit sys.jobs tx")?;

    Ok(partition_job_statuses(rows))
}

fn partition_job_statuses(rows: Vec<(String, String)>) -> (Vec<String>, Vec<String>) {
    let mut active = Vec::new();
    let mut failed = Vec::new();
    for (job_id, status) in rows {
        match status.as_str() {
            "submitted" | "processing" | "running" => active.push(job_id),
            "failed" => failed.push(job_id),
            _ => {}
        }
    }
    (active, failed)
}

async fn wait_for_dsql_job(pool: &PgPool, job_id: &str) -> Result<()> {
    let row = sqlx::query("CALL sys.wait_for_job($1)")
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("CALL sys.wait_for_job failed for job `{job_id}`"))?;

    let Some(row) = row else {
        return Ok(());
    };
    let succeeded: bool = row.try_get(0).with_context(|| {
        format!("Unexpected return column from sys.wait_for_job for `{job_id}`")
    })?;
    if !succeeded {
        bail!("Aurora DSQL async job `{job_id}` finished unsuccessfully");
    }
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use sqlx::migrate::MigrationType;

    use super::*;

    #[test]
    fn dsql_migrator_disables_advisory_locking_and_validates_invariants() {
        let migrator = dsql_migrator();
        assert!(!migrator.locking);
        assert_eq!(migrator.iter().len(), 34);
        assert!(validate_dsql_invariants(&migrator).is_ok());
    }

    #[test]
    fn validate_dsql_invariants_rejects_empty_and_transactional_migrations() {
        let empty = Migrator::with_migrations(Vec::new());
        assert!(validate_dsql_invariants(&empty).is_err());

        let tx_migration = Migration::new(
            1,
            Cow::Borrowed("create_foo"),
            MigrationType::Simple,
            "CREATE TABLE foo (id UUID PRIMARY KEY);".into_sql_str(),
            false,
        );
        let invalid = Migrator::with_migrations(vec![tx_migration]);
        assert!(validate_dsql_invariants(&invalid).is_err());
    }

    #[test]
    fn rewrite_sql_for_postgres_strips_async_keyword_cleanly() {
        let regular = "-- no-transaction\nCREATE INDEX ASYNC IF NOT EXISTS idx_a ON tbl (col);";
        assert_eq!(
            rewrite_sql_for_postgres(regular),
            "-- no-transaction\nCREATE INDEX IF NOT EXISTS idx_a ON tbl (col);"
        );

        let unique = "CREATE UNIQUE INDEX ASYNC idx_b ON tbl (col);";
        assert_eq!(
            rewrite_sql_for_postgres(unique),
            "CREATE UNIQUE INDEX idx_b ON tbl (col);"
        );

        let table = "CREATE TABLE IF NOT EXISTS async_log (id UUID PRIMARY KEY);";
        assert_eq!(rewrite_sql_for_postgres(table), table);
    }

    #[test]
    fn partition_job_statuses_recognizes_dsql_active_and_failed_states() {
        let rows = vec![
            ("job-1".to_string(), "submitted".to_string()),
            ("job-2".to_string(), "processing".to_string()),
            ("job-3".to_string(), "running".to_string()),
            ("job-4".to_string(), "failed".to_string()),
            ("job-5".to_string(), "completed".to_string()),
        ];
        let (active, failed) = partition_job_statuses(rows);
        assert_eq!(active, vec!["job-1", "job-2", "job-3"]);
        assert_eq!(failed, vec!["job-4"]);
    }

    #[test]
    fn occ_backoff_delay_grows_exponentially_and_caps() {
        assert_eq!(occ_backoff_delay(1), Duration::from_millis(50));
        assert_eq!(occ_backoff_delay(2), Duration::from_millis(100));
        assert_eq!(occ_backoff_delay(3), Duration::from_millis(200));
        assert_eq!(occ_backoff_delay(10), Duration::from_millis(1_000));
    }
}
