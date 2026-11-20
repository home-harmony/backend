//! Authentication claims and Axum extractor (ADR-0011).
//!
//! ## How authentication and multi-family context work
//!
//! 1. API Gateway validates the Cognito JWT signature before the request reaches Lambda.
//! 2. Auth middleware extracts `user_id` (`sub`) and `email` from the validated JWT claims.
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
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};

use domain::value_objects::{Email, FamilyId, Permission, Role, UserId};

/// Authenticated caller and verified family workspace context.
///
/// Populated by auth middleware from the validated Cognito JWT (`user_id`, `email`)
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

    /// User's validated and normalized email address from the Cognito JWT `email` claim.
    pub email: Email,
}

/// Axum extractor that reads `AuthClaims` from request extensions.
///
/// Claims are inserted into request extensions by the `auth_middleware`.
/// Handlers simply declare `Extension(claims): Extension<AuthClaims>` to receive them.
///
/// # Example
///
/// ```rust,no_run
/// use axum::{Extension, Json};
/// use api::auth::AuthClaims;
///
/// async fn get_family(Extension(claims): Extension<AuthClaims>) -> Json<String> {
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
        let (status, message) = match &self {
            AuthError::MissingClaims => (StatusCode::UNAUTHORIZED, self.to_string()),
            AuthError::InvalidClaims(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            AuthError::Forbidden(_) => (StatusCode::FORBIDDEN, self.to_string()),
        };

        #[derive(Serialize)]
        struct Body {
            error: String,
        }

        (status, Json(Body { error: message })).into_response()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_claims_json(email: &str) -> serde_json::Value {
        serde_json::json!({
            "user_id": UserId::new_v4(),
            "family_id": FamilyId::new_v4(),
            "role": "member",
            "permissions": [],
            "email": email
        })
    }

    #[test]
    fn auth_claims_serde_normalizes_and_round_trips_email() {
        let claims: AuthClaims =
            serde_json::from_value(sample_claims_json("  Alice@Example.COM  ")).unwrap();
        assert_eq!(claims.email.as_ref(), "alice@example.com");

        let serialized = serde_json::to_value(&claims).unwrap();
        assert_eq!(serialized["email"], "alice@example.com");
    }

    #[test]
    fn auth_claims_rejects_invalid_or_empty_email() {
        assert!(serde_json::from_value::<AuthClaims>(sample_claims_json("")).is_err());
        assert!(serde_json::from_value::<AuthClaims>(sample_claims_json("not-an-email")).is_err());
    }

    #[test]
    fn auth_error_maps_to_expected_http_statuses() {
        assert_eq!(
            AuthError::MissingClaims.into_response().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            AuthError::InvalidClaims("bad token".to_string())
                .into_response()
                .status(),
            StatusCode::UNAUTHORIZED
        );

        let forbidden = AuthError::Forbidden(Role::Child);
        assert_eq!(
            forbidden.to_string(),
            "Insufficient permissions for role: child"
        );
        assert_eq!(forbidden.into_response().status(), StatusCode::FORBIDDEN);
    }
}
