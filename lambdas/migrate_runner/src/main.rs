//! # Migration Runner Lambda
//!
//! This Lambda function connects to Aurora DSQL and runs all embedded SQL
//! migration files via `sqlx::migrate!()`. It is triggered manually at deploy
//! time to initialize or update the database schema.
//!
//! ## Deployment
//!
//! ```powershell
//! # Build for ARM64 Lambda
//! cargo lambda build --release --arm64
//!
//! # Trigger after deploying the SAM stack
//! aws lambda invoke --function-name familyledger-migrate --payload '{}' response.json
//!
//! # Check the result
//! cat response.json
//! ```
//!
//! ## Environment Variables
//!
//! - `DSQL_ENDPOINT`: Aurora DSQL cluster hostname
//!   Example: `mydb.dsql.us-east-1.on.aws`
//!
//! ## Migration Rules (Aurora DSQL)
//!
//! Each file under `migrations/` contains **exactly one DDL statement**.
//! This is required by Aurora DSQL which only allows one DDL operation per transaction.
//! See `documentation/roadmap/sprint-1/task_3_1_detailed_guide.md` for the full list of migration files.

use anyhow::Context;
use infrastructure::db::pool::create_pool;
use lambda_runtime::{Error, LambdaEvent, run, service_fn};
use serde::{Deserialize, Serialize};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

// ─── Embedded Migrations ──────────────────────────────────────────────────────
// `sqlx::migrate!` embeds all SQL files from the `migrations/` directory into
// the compiled binary at build time. The path is relative to the crate manifest.
// At runtime, no filesystem access is needed — migrations live inside the binary.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

// ─── Lambda Request/Response ──────────────────────────────────────────────────

/// Input payload (empty — this Lambda takes no parameters).
#[derive(Deserialize)]
struct Request {}

/// Output payload returned after successful migration.
#[derive(Serialize)]
struct Response {
    message: String,
    /// Number of migration files embedded in the binary.
    /// sqlx skips already-applied migrations based on its `_sqlx_migrations` tracking table.
    total_migrations: usize,
}

// ─── Entry Point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Initialize structured JSON logging for CloudWatch.
    // Set RUST_LOG=info in the Lambda environment to see logs.
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().json())
        .init();

    // lambda_runtime 1.x: `run` accepts a tower Service or a closure via `service_fn`.
    run(service_fn(handler)).await
}

// ─── Handler ──────────────────────────────────────────────────────────────────

async fn handler(_event: LambdaEvent<Request>) -> Result<Response, Error> {
    let pg_pool = create_pool().await?;

    info!("Running schema migrations");

    // Apply all pending migrations.
    // sqlx tracks applied migrations in the `_sqlx_migrations` table.
    // Already-applied migrations are skipped automatically — safe to call on every deploy.
    MIGRATOR
        .run(&pg_pool)
        .await
        .context("Migration run failed")?;

    let total_migrations = MIGRATOR.iter().count();

    info!(
        count = total_migrations,
        "All migrations applied successfully"
    );

    Ok(Response {
        message: "Schema migrations completed successfully".to_string(),
        total_migrations,
    })
}
