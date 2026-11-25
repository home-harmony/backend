//! # Migration Runner Lambda
//!
//! Connects to Amazon Aurora DSQL and applies all embedded SQL migrations
//! via `infrastructure::db::migrations`. Invoked manually or from CI/CD after
//! SAM deployment to initialize or advance the schema.
//!
//! ## Deployment & Invocation
//!
//! ```bash
//! # Build for ARM64 Lambda (outputs target/lambda/migrate_runner/bootstrap)
//! cargo lambda build --release --arm64 --package migrate_runner
//!
//! # Deploy stack & invoke after deploy
//! aws lambda invoke --function-name familyledger-migrate-dev --payload '{}' response.json
//! cat response.json
//! ```
//!
//! ## Environment Variables
//!
//! - `DSQL_ENDPOINT`: Aurora DSQL cluster hostname or `postgres://` URL
//!   Example: `exampleclusterid1234567890.dsql.us-east-1.on.aws`
//! - `DSQL_USER` *(optional)*: DSQL user (`admin` by default)
//!
//! ## Migration Rules (Aurora DSQL)
//!
//! Each file under `migrations/` starts with `-- no-transaction` and contains
//! **exactly one DDL statement**.

use anyhow::Context;
use lambda_runtime::{Error, LambdaEvent, run, service_fn};
use serde::{Deserialize, Serialize};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use infrastructure::db::{
    migrations::{DsqlMigrationOptions, MigrationReport, run_dsql_migrations},
    pool::{PgPool, create_admin_pool},
};

const MIGRATION_POOL_MAX_CONNECTIONS: u32 = 1;

/// Input payload (`{}` uses defaults: wait for `CREATE INDEX ASYNC` jobs).
#[derive(Debug, Deserialize)]
struct Request {
    #[serde(default = "default_wait_for_indexes")]
    wait_for_indexes: bool,
}

/// Output payload returned after migration execution.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct Response {
    message: String,
    total_migrations: usize,
    applied_now: usize,
    already_applied: usize,
    active_async_jobs: usize,
}

impl Response {
    fn from_report(report: MigrationReport) -> Self {
        Self {
            message: "Schema migrations completed successfully".to_string(),
            total_migrations: report.total_migrations,
            applied_now: report.applied_now,
            already_applied: report.already_applied,
            active_async_jobs: report.active_async_jobs,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().json())
        .init();

    let pg_pool = create_admin_pool(MIGRATION_POOL_MAX_CONNECTIONS).await?;
    run(service_fn(|event| handler(&pg_pool, event))).await
}

async fn handler(pg_pool: &PgPool, event: LambdaEvent<Request>) -> Result<Response, Error> {
    let options = DsqlMigrationOptions {
        wait_for_indexes: event.payload.wait_for_indexes,
        ..DsqlMigrationOptions::default()
    };

    info!(
        wait_for_indexes = options.wait_for_indexes,
        "Running Aurora DSQL schema migrations"
    );

    let report = run_dsql_migrations(pg_pool, options)
        .await
        .context("Migration run failed")?;

    info!(
        total_migrations = report.total_migrations,
        applied_now = report.applied_now,
        already_applied = report.already_applied,
        active_async_jobs = report.active_async_jobs,
        "Schema migrations completed"
    );

    Ok(Response::from_report(report))
}

const fn default_wait_for_indexes() -> bool {
    true
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_defaults_to_waiting_for_async_indexes() {
        let req: Request = serde_json::from_str("{}").unwrap();
        assert!(req.wait_for_indexes);
    }

    #[test]
    fn explicit_payload_overrides_wait_for_indexes() {
        let req: Request = serde_json::from_str(r#"{"wait_for_indexes": false}"#).unwrap();
        assert!(!req.wait_for_indexes);
    }

    #[test]
    fn response_serializes_migration_report_fields() {
        let report = MigrationReport {
            total_migrations: 34,
            applied_now: 34,
            already_applied: 0,
            active_async_jobs: 0,
        };
        let json = serde_json::to_value(Response::from_report(report)).unwrap();
        assert_eq!(json["total_migrations"], 34);
        assert_eq!(json["applied_now"], 34);
        assert_eq!(json["already_applied"], 0);
        assert_eq!(json["active_async_jobs"], 0);
        assert_eq!(json["message"], "Schema migrations completed successfully");
    }
}
