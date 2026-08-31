//! Aurora DSQL connection pool setup.
//!
//! Uses `aurora-dsql-sqlx-connector` which handles:
//! - IAM token generation (no passwords — AWS IAM auth only)
//! - Token refresh in the background (every ~12 minutes for a 15-min token)
//! - OCC retry on commit conflict (SQLSTATE 40001)
//!
//! # Usage
//!
//! ```rust,no_run
//! use infrastructure::db::pool::create_pool;
//!
//! #[tokio::main]
//! async fn main() {
//!     let pool = create_pool().await.expect("Failed to connect to Aurora DSQL");
//!     // Use pool for queries...
//! }
//! ```

use anyhow::{Context, Result};
use aurora_dsql_sqlx_connector::pool;
use sqlx::PgPool;
use tracing::info;

/// Creates and returns an Aurora DSQL connection pool.
///
/// Reads the `DSQL_ENDPOINT` environment variable for the cluster hostname.
/// The connector auto-detects the AWS region from the hostname suffix.
///
/// # Environment Variables
///
/// - `DSQL_ENDPOINT`: Aurora DSQL cluster endpoint hostname.
///   Example: `mydb.dsql.us-east-1.on.aws`
///
/// # Errors
///
/// Returns an error if:
/// - `DSQL_ENDPOINT` is not set
/// - IAM token generation fails (check Lambda execution role permissions)
/// - Connection to the cluster fails
pub async fn create_pool() -> Result<PgPool> {
    let endpoint =
        std::env::var("DSQL_ENDPOINT").context("DSQL_ENDPOINT environment variable is required")?;

    info!(endpoint = %endpoint, "Connecting to Aurora DSQL");

    let pg_pool = pool::connect(&endpoint)
        .await
        .context("Failed to connect to Aurora DSQL")?;

    info!("Aurora DSQL connection pool established");
    Ok(pg_pool)
}
