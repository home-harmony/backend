//! Aurora DSQL connection pool setup.
//!
//! Uses `aurora-dsql-sqlx-connector` which handles:
//! - IAM token generation (no passwords — AWS IAM auth only)
//! - Token refresh in the background (every ~12 minutes for a 15-min token)
//! - OCC retry on commit conflict (`SQLSTATE 40001` / `OC000` / `OC001`)
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

use anyhow::{Context, Result, bail};
use aurora_dsql_sqlx_connector::{DsqlConnectOptions, pool};
pub use sqlx::postgres::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::info;

const DEFAULT_DSQL_USER: &str = "admin";
const DEFAULT_DSQL_PORT: u16 = 5432;
const DEFAULT_DSQL_DATABASE: &str = "postgres";
const DEFAULT_LAMBDA_MAX_CONNECTIONS: u32 = 5;

/// Creates and returns an Aurora DSQL connection pool using the default Lambda pool size.
///
/// Requires `DSQL_ENDPOINT` and `DSQL_USER` (must not be `"admin"`, enforcing least privilege
/// for runtime service Lambdas operating under `FamilyLedgerServiceRole`).
///
/// # Errors
///
/// Returns an error if `DSQL_ENDPOINT` or `DSQL_USER` is missing/invalid, `DSQL_USER == "admin"`,
/// IAM token generation fails, or establishing the pool fails.
pub async fn create_pool() -> Result<PgPool> {
    create_service_pool(DEFAULT_LAMBDA_MAX_CONNECTIONS).await
}

/// Creates and returns an Aurora DSQL service connection pool capped at `max_connections`.
///
/// Rejects unset `DSQL_USER` or `"admin"` so runtime services never accidentally connect
/// with administrative privileges.
///
/// # Errors
///
/// Returns an error if `max_connections == 0`, `DSQL_ENDPOINT` or `DSQL_USER` is missing/invalid,
/// `DSQL_USER` is `"admin"`, IAM token generation fails, or establishing the pool fails.
pub async fn create_service_pool(max_connections: u32) -> Result<PgPool> {
    let user = std::env::var("DSQL_USER")
        .context("DSQL_USER environment variable is required for service pools")?;
    let user = user.trim();
    if user.is_empty() || user.eq_ignore_ascii_case(DEFAULT_DSQL_USER) {
        bail!("Runtime service pools must set DSQL_USER to a non-admin database role");
    }
    connect_pool_with_user(max_connections, user, false).await
}

/// Creates and returns an administrative Aurora DSQL connection pool capped at `max_connections`.
///
/// Intended exclusively for `migrate_runner` (`dsql:DbConnectAdmin`). Reads `DSQL_USER`
/// and defaults to `"admin"` when unset.
///
/// # Errors
///
/// Returns an error if `max_connections == 0`, `DSQL_ENDPOINT` is missing/invalid,
/// IAM token generation fails, or establishing the pool fails.
pub async fn create_admin_pool(max_connections: u32) -> Result<PgPool> {
    let user = std::env::var("DSQL_USER").unwrap_or_else(|_| DEFAULT_DSQL_USER.to_string());
    connect_pool_with_user(max_connections, &user, true).await
}

/// Alias for [`create_admin_pool`] preserved for administrative callers.
///
/// # Errors
///
/// Returns an error if `max_connections == 0`, `DSQL_ENDPOINT` is missing/invalid,
/// IAM token generation fails, or establishing the pool fails.
pub async fn create_pool_with_max_connections(max_connections: u32) -> Result<PgPool> {
    create_admin_pool(max_connections).await
}

/// Redacts userinfo (`user:password@`) and query/fragment parameters before logging `endpoint`.
#[must_use]
pub fn sanitize_endpoint_for_log(endpoint: &str) -> String {
    let trimmed = endpoint.trim();
    let (scheme_prefix, rest) = match trimmed.split_once("://") {
        Some((scheme, after)) => (Some(scheme), after),
        None => (None, trimmed),
    };
    let without_query = rest.split(['?', '#']).next().unwrap_or("");
    let host_and_path = match without_query.rsplit_once('@') {
        Some((_, after_at)) => after_at,
        None => without_query,
    };
    match scheme_prefix {
        Some(scheme) => format!("{scheme}://{host_and_path}"),
        None => host_and_path.to_owned(),
    }
}

/// Normalizes a bare DSQL cluster hostname or a `postgres://` / `postgresql://` URL
/// into a connection URL accepted by `aurora-dsql-sqlx-connector`.
///
/// `aurora_dsql_sqlx_connector::pool::connect` requires a `postgres://` or `postgresql://`
/// scheme, whereas SAM environment templates typically supply bare `DSQL_ENDPOINT` hostnames.
///
/// # Errors
///
/// Returns an error if `endpoint` or `user` is empty, uses an unsupported scheme,
/// or contains invalid bare-hostname characters.
pub fn normalize_dsql_url(endpoint: &str, user: &str) -> Result<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        bail!("DSQL_ENDPOINT must not be empty");
    }

    if let Some((scheme, rest)) = endpoint.split_once("://") {
        if scheme != "postgres" && scheme != "postgresql" {
            bail!(
                "Unsupported DSQL URL scheme `{scheme}`; expected `postgres://` or `postgresql://`"
            );
        }
        if rest.trim().is_empty() {
            bail!("DSQL URL is missing host component");
        }
        return Ok(endpoint.to_string());
    }

    build_url_from_host(endpoint, user)
}

async fn connect_pool_with_user(
    max_connections: u32,
    user: &str,
    allow_admin: bool,
) -> Result<PgPool> {
    if max_connections == 0 {
        bail!("max_connections must be at least 1");
    }

    let endpoint =
        std::env::var("DSQL_ENDPOINT").context("DSQL_ENDPOINT environment variable is required")?;
    let conn_str = normalize_dsql_url(&endpoint, user)?;
    if !allow_admin {
        ensure_non_admin_service_url(&conn_str)?;
    }
    let sanitized_endpoint = sanitize_endpoint_for_log(&endpoint);

    info!(endpoint = %sanitized_endpoint, max_connections, "Connecting to Aurora DSQL");

    let config = DsqlConnectOptions::from_connection_string(&conn_str)
        .context("Invalid Aurora DSQL connection configuration")?;
    let pool_options = PgPoolOptions::new().max_connections(max_connections);
    let pg_pool = pool::connect_with(&config, pool_options)
        .await
        .context("Failed to connect to Aurora DSQL")?;

    info!("Aurora DSQL connection pool established");
    Ok(pg_pool)
}

fn ensure_non_admin_service_url(conn_str: &str) -> Result<()> {
    let Some((_, rest)) = conn_str.split_once("://") else {
        return Ok(());
    };
    let Some((userinfo, _)) = rest.split_once('@') else {
        return Ok(());
    };
    let username = userinfo.split(':').next().unwrap_or("");
    if username.eq_ignore_ascii_case(DEFAULT_DSQL_USER) {
        bail!("Runtime service pools must not embed admin user in DSQL_ENDPOINT");
    }
    Ok(())
}

fn build_url_from_host(host: &str, user: &str) -> Result<String> {
    let user = user.trim();
    if user.is_empty()
        || user.chars().any(char::is_whitespace)
        || user.contains(['/', '@', ':', '?', '#'])
    {
        bail!("Invalid DSQL_USER `{user}`");
    }
    if host.chars().any(char::is_whitespace) || host.contains(['/', '@', ':', '?', '#']) {
        bail!("Invalid bare DSQL hostname `{host}`");
    }
    Ok(format!(
        "postgres://{user}@{host}:{DEFAULT_DSQL_PORT}/{DEFAULT_DSQL_DATABASE}"
    ))
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_hostname_defaults_to_postgres_admin_url_and_parses() {
        let url = normalize_dsql_url("exampleclusterid1234567890.dsql.us-east-1.on.aws", "admin")
            .unwrap();
        assert_eq!(
            url,
            "postgres://admin@exampleclusterid1234567890.dsql.us-east-1.on.aws:5432/postgres"
        );
        assert!(DsqlConnectOptions::from_connection_string(&url).is_ok());
    }

    #[test]
    fn bare_hostname_uses_custom_user_and_trims_whitespace() {
        let url =
            normalize_dsql_url("  cluster.dsql.us-east-1.on.aws  ", "  service_user ").unwrap();
        assert_eq!(
            url,
            "postgres://service_user@cluster.dsql.us-east-1.on.aws:5432/postgres"
        );
        assert!(DsqlConnectOptions::from_connection_string(&url).is_ok());
    }

    #[test]
    fn existing_postgres_and_postgresql_urls_pass_through() {
        let pg_url =
            "postgres://admin@cluster.dsql.us-east-1.on.aws:5432/postgres?region=us-east-1";
        assert_eq!(normalize_dsql_url(pg_url, "ignored").unwrap(), pg_url);

        let pgsql_url = "postgresql://svc@cluster.dsql.us-east-1.on.aws/postgres";
        assert_eq!(normalize_dsql_url(pgsql_url, "ignored").unwrap(), pgsql_url);
    }

    #[test]
    fn rejects_empty_invalid_scheme_and_malformed_host() {
        assert!(normalize_dsql_url("", "admin").is_err());
        assert!(normalize_dsql_url("   ", "admin").is_err());
        assert!(normalize_dsql_url("cluster.dsql.us-east-1.on.aws", "   ").is_err());
        assert!(normalize_dsql_url("cluster.dsql.us-east-1.on.aws", "bad@user").is_err());
        assert!(normalize_dsql_url("http://cluster.dsql.us-east-1.on.aws", "admin").is_err());
        assert!(normalize_dsql_url("postgres://", "admin").is_err());
        assert!(normalize_dsql_url("bad host.dsql.us-east-1.on.aws", "admin").is_err());
        assert!(normalize_dsql_url("cluster.dsql.us-east-1.on.aws:5432", "admin").is_err());
        assert!(normalize_dsql_url("admin@cluster.dsql.us-east-1.on.aws", "admin").is_err());
    }

    #[test]
    fn sanitize_endpoint_for_log_strips_userinfo_and_query_params() {
        assert_eq!(
            sanitize_endpoint_for_log(
                "postgres://admin:supersecret@cluster.dsql.us-east-1.on.aws:5432/postgres?sslmode=require"
            ),
            "postgres://cluster.dsql.us-east-1.on.aws:5432/postgres"
        );
        assert_eq!(
            sanitize_endpoint_for_log("  cluster.dsql.us-east-1.on.aws  "),
            "cluster.dsql.us-east-1.on.aws"
        );
    }

    #[test]
    fn ensure_non_admin_service_url_rejects_embedded_admin_user() {
        assert!(
            ensure_non_admin_service_url(
                "postgres://admin@cluster.dsql.us-east-1.on.aws:5432/postgres"
            )
            .is_err()
        );
        assert!(
            ensure_non_admin_service_url(
                "postgres://familyledger_app@cluster.dsql.us-east-1.on.aws:5432/postgres"
            )
            .is_ok()
        );
    }
}
