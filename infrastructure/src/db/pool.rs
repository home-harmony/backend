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
use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::info;

const DEFAULT_DSQL_USER: &str = "admin";
const DEFAULT_DSQL_PORT: u16 = 5432;
const DEFAULT_DSQL_DATABASE: &str = "postgres";
const DEFAULT_LAMBDA_MAX_CONNECTIONS: u32 = 5;

/// Creates and returns an Aurora DSQL connection pool using the default Lambda pool size.
///
/// Reads `DSQL_ENDPOINT` (required, either bare hostname or full `postgres://` URL) and
/// `DSQL_USER` (optional, defaults to `"admin"` for admin utilities such as `migrate_runner`).
/// Least-privilege runtime services (`dsql:DbConnect` without `dsql:DbConnectAdmin`) must set
/// `DSQL_USER` (or embed the non-admin user in `DSQL_ENDPOINT`) to a custom database role.
///
/// # Errors
///
/// Returns an error if `DSQL_ENDPOINT` is missing/invalid, IAM token generation fails,
/// or establishing the pool fails.
pub async fn create_pool() -> Result<PgPool> {
    create_pool_with_max_connections(DEFAULT_LAMBDA_MAX_CONNECTIONS).await
}

/// Creates and returns an Aurora DSQL connection pool capped at `max_connections`.
///
/// # Errors
///
/// Returns an error if `max_connections == 0`, `DSQL_ENDPOINT` is missing/invalid,
/// IAM token generation fails, or establishing the pool fails.
pub async fn create_pool_with_max_connections(max_connections: u32) -> Result<PgPool> {
    let endpoint =
        std::env::var("DSQL_ENDPOINT").context("DSQL_ENDPOINT environment variable is required")?;
    let user = std::env::var("DSQL_USER").unwrap_or_else(|_| DEFAULT_DSQL_USER.to_string());
    let conn_str = normalize_dsql_url(&endpoint, &user)?;
    connect_dsql_pool(&conn_str, &endpoint, max_connections).await
}

/// Creates a least-privilege Aurora DSQL connection pool for runtime service Lambdas.
///
/// Enforces a non-empty, non-`"admin"` database role via `DSQL_USER` or the URL user.
///
/// # Errors
///
/// Returns an error if `DSQL_ENDPOINT` is missing/invalid, the resolved role is `"admin"` or
/// unset, IAM token generation fails, or establishing the pool fails.
pub async fn create_service_pool() -> Result<PgPool> {
    create_service_pool_with_max_connections(DEFAULT_LAMBDA_MAX_CONNECTIONS).await
}

/// Creates a least-privilege Aurora DSQL connection pool capped at `max_connections`.
///
/// # Errors
///
/// Returns an error if `max_connections == 0`, `DSQL_ENDPOINT` is missing/invalid,
/// the resolved role is `"admin"` or unset, or establishing the pool fails.
pub async fn create_service_pool_with_max_connections(max_connections: u32) -> Result<PgPool> {
    let endpoint =
        std::env::var("DSQL_ENDPOINT").context("DSQL_ENDPOINT environment variable is required")?;
    let dsql_user = std::env::var("DSQL_USER").ok();
    let conn_str = resolve_service_dsql_url(&endpoint, dsql_user.as_deref())?;
    connect_dsql_pool(&conn_str, &endpoint, max_connections).await
}

/// Resolves a runtime service connection string, rejecting `"admin"` or missing non-admin role.
///
/// # Errors
///
/// Returns an error if `endpoint` is invalid, no non-admin role is configured, or the resolved
/// user is `"admin"`.
pub fn resolve_service_dsql_url(endpoint: &str, dsql_user: Option<&str>) -> Result<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        bail!("DSQL_ENDPOINT must not be empty");
    }
    if endpoint.contains("://") {
        let normalized = normalize_dsql_url(endpoint, "")?;
        let url_user = extract_url_user(&normalized)?;
        ensure_non_admin_user(url_user)?;
        return Ok(normalized);
    }
    let user = dsql_user
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .context("DSQL_USER is required for runtime services and must not be 'admin'")?;
    ensure_non_admin_user(user)?;
    build_url_from_host(endpoint, user)
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

async fn connect_dsql_pool(conn_str: &str, endpoint: &str, max_connections: u32) -> Result<PgPool> {
    if max_connections == 0 {
        bail!("max_connections must be at least 1");
    }
    info!(endpoint = %endpoint, max_connections, "Connecting to Aurora DSQL");

    let config = DsqlConnectOptions::from_connection_string(conn_str)
        .context("Invalid Aurora DSQL connection configuration")?;
    let pool_options = PgPoolOptions::new().max_connections(max_connections);
    let pg_pool = pool::connect_with(&config, pool_options)
        .await
        .context("Failed to connect to Aurora DSQL")?;

    info!("Aurora DSQL connection pool established");
    Ok(pg_pool)
}

fn extract_url_user(url: &str) -> Result<&str> {
    let Some((_, rest)) = url.split_once("://") else {
        bail!("Invalid DSQL URL");
    };
    let authority = rest.split(['/', '?']).next().unwrap_or("");
    let Some((userinfo, host)) = authority.split_once('@') else {
        bail!("Service DSQL URL must specify a non-admin user");
    };
    if host.trim().is_empty() {
        bail!("DSQL URL is missing host component");
    }
    Ok(userinfo.split(':').next().unwrap_or("").trim())
}

fn ensure_non_admin_user(user: &str) -> Result<()> {
    if user.is_empty() || user.eq_ignore_ascii_case(DEFAULT_DSQL_USER) {
        bail!("Runtime service DSQL user must be a non-empty, non-admin IAM role");
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
    fn service_dsql_url_accepts_non_admin_role_for_host_and_url() {
        let host_url =
            resolve_service_dsql_url("cluster.dsql.us-east-1.on.aws", Some("family_svc")).unwrap();
        assert_eq!(
            host_url,
            "postgres://family_svc@cluster.dsql.us-east-1.on.aws:5432/postgres"
        );

        let full_url = "postgres://family_svc@cluster.dsql.us-east-1.on.aws:5432/postgres";
        assert_eq!(resolve_service_dsql_url(full_url, None).unwrap(), full_url);
    }

    #[test]
    fn service_dsql_url_rejects_missing_empty_or_admin_role() {
        let host = "cluster.dsql.us-east-1.on.aws";
        assert!(resolve_service_dsql_url(host, None).is_err());
        assert!(resolve_service_dsql_url(host, Some("")).is_err());
        assert!(resolve_service_dsql_url(host, Some("   ")).is_err());
        assert!(resolve_service_dsql_url(host, Some("admin")).is_err());
        assert!(resolve_service_dsql_url(host, Some("ADMIN")).is_err());
        assert!(
            resolve_service_dsql_url(
                "postgres://admin@cluster.dsql.us-east-1.on.aws/postgres",
                None
            )
            .is_err()
        );
        assert!(
            resolve_service_dsql_url(
                "postgres://cluster.dsql.us-east-1.on.aws/postgres",
                Some("svc")
            )
            .is_err()
        );
    }
}
