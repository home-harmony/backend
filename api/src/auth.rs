//! Authentication claims and Axum extractor (ADR-0011).
//!
//! ## How authentication and multi-family context work
//!
//! 1. API Gateway validates the Cognito JWT signature before the request reaches Lambda.
//! 2. Auth middleware extracts `user_id` (`sub`) and optional `email` from the validated JWT claims
//!    (Access Tokens carry `sub`; ID Tokens also include `email`).
//! 3. For family-scoped requests, the client sends the active workspace via the `X-Family-Id`
//!    HTTP header. Auth middleware verifies active membership in `family_members` for
//!    `(family_id, user_id)` and resolves `role` and `permissions` before inserting `AuthClaims`.
//!
//! ## RULE (AGENTS.md & ADR-0011)
//!
//! `family_id` is **NEVER** trusted from unverified client request bodies or query params.
//! It is extracted from `X-Family-Id` and strictly verified against `family_members` in Aurora DSQL.

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use domain::value_objects::{Email, FamilyId, Permission, Role, UserId};

use crate::errors::ApiError;

/// Authenticated caller and verified family workspace context.
///
/// Populated by auth middleware from the validated Cognito JWT (`user_id`, optional `email`)
/// and verified `X-Family-Id` membership (`family_id`, `role`, `permissions`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
    /// Cognito `sub` — the user's permanent unique identifier.
    /// Stored as `user_id` in the `family_members` table.
    pub user_id: UserId,

    /// Verified active family workspace ID from the `X-Family-Id` header.
    ///
    /// RULE: This value is the authoritative `family_id` for all queries.
    /// Never use a `family_id` from the request body or query parameters.
    pub family_id: FamilyId,

    /// The user's resolved role within `family_id` (from `family_members.role`).
    pub role: Role,

    /// Granular permission flags for `Role::Other` members (from `family_members.permissions`).
    pub permissions: Vec<Permission>,

    /// User's validated and normalized email address when present in the JWT (Cognito ID tokens).
    /// `None` for Cognito Access Tokens or email-less federated logins.
    #[serde(default)]
    pub email: Option<Email>,
}

/// Axum extractor that reads `AuthClaims` from request extensions.
///
/// Claims are inserted into request extensions by the `auth_middleware`.
/// Handlers declare `claims: AuthClaims` to receive them via [`FromRequestParts`].
///
/// # Example
///
/// ```rust,no_run
/// use axum::Json;
/// use api::auth::AuthClaims;
///
/// async fn get_family(claims: AuthClaims) -> Json<String> {
///     // claims.family_id is safe to use — verified against family_members
///     Json(claims.family_id.to_string())
/// }
/// ```
impl<S> FromRequestParts<S> for AuthClaims
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthClaims>()
            .cloned()
            .ok_or(AuthError::MissingClaims)
    }
}

/// Errors that can occur during auth claim extraction.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Authentication claims not found in request context")]
    MissingClaims,

    #[error("Invalid JWT claims: {0}")]
    InvalidClaims(String),

    #[error("Insufficient permissions for role: {0}")]
    Forbidden(Role),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AuthError::MissingClaims => (StatusCode::UNAUTHORIZED, "MISSING_CLAIMS"),
            AuthError::InvalidClaims(_) => (StatusCode::UNAUTHORIZED, "INVALID_CLAIMS"),
            AuthError::Forbidden(_) => (StatusCode::FORBIDDEN, "FORBIDDEN"),
        };
        ApiError::problem(status, code, self.to_string()).into_response()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::header;

    fn sample_claims_json(email: Option<&str>) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "user_id": UserId::new_v4(),
            "family_id": FamilyId::new_v4(),
            "role": "member",
            "permissions": []
        });
        if let Some(addr) = email {
            obj["email"] = serde_json::json!(addr);
        }
        obj
    }

    #[test]
    fn auth_claims_serde_normalizes_and_round_trips_optional_email() {
        let with_email: AuthClaims =
            serde_json::from_value(sample_claims_json(Some("  Alice@Example.COM  "))).unwrap();
        assert_eq!(
            with_email.email.as_ref().map(Email::as_str),
            Some("alice@example.com")
        );

        let serialized = serde_json::to_value(&with_email).unwrap();
        assert_eq!(serialized["email"], "alice@example.com");

        let access_token_claims: AuthClaims =
            serde_json::from_value(sample_claims_json(None)).unwrap();
        assert!(access_token_claims.email.is_none());
    }

    #[test]
    fn auth_claims_rejects_invalid_or_empty_email() {
        assert!(serde_json::from_value::<AuthClaims>(sample_claims_json(Some(""))).is_err());
        assert!(
            serde_json::from_value::<AuthClaims>(sample_claims_json(Some("not-an-email"))).is_err()
        );
    }

    #[tokio::test]
    async fn auth_error_maps_to_problem_json_with_expected_statuses_and_codes() {
        let cases = [
            (
                AuthError::MissingClaims,
                StatusCode::UNAUTHORIZED,
                "MISSING_CLAIMS",
            ),
            (
                AuthError::InvalidClaims("bad token".to_string()),
                StatusCode::UNAUTHORIZED,
                "INVALID_CLAIMS",
            ),
            (
                AuthError::Forbidden(Role::Child),
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
            ),
        ];

        for (err, expected_status, expected_code) in cases {
            let resp = err.into_response();
            assert_eq!(resp.status(), expected_status);
            assert_eq!(
                resp.headers().get(header::CONTENT_TYPE).unwrap(),
                "application/problem+json"
            );
            let bytes = to_bytes(resp.into_body(), 16 * 1024).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["code"], expected_code);
            assert_eq!(body["status"], expected_status.as_u16());
        }
    }
}
