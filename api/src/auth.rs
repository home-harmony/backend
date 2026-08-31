//! JWT authentication claims and Axum extractor.
//!
//! ## How JWT validation works in this architecture
//!
//! API Gateway validates the JWT signature before the request reaches Lambda.
//! The Lambda handler only needs to decode the (already-trusted) claims from
//! the `requestContext.authorizer.jwt.claims` field in the Lambda event.
//!
//! We still parse the JWT to extract `sub` (user ID), `custom:family_id`,
//! and `custom:family_role` from the claims.
//!
//! ## RULE (GEMINI.md §9)
//!
//! `family_id` is **NEVER** trusted from client request bodies or query params.
//! It is **always** extracted from the JWT claims here.

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Json, Response},
};
use domain::value_objects::{FamilyId, Role, UserId};
use serde::{Deserialize, Serialize};

/// Claims extracted from the Cognito JWT.
///
/// These are populated by API Gateway from the validated JWT before invoking Lambda.
/// The handler receives them pre-validated — no signature verification needed here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
    /// Cognito `sub` — the user's permanent unique identifier.
    /// This is stored as `user_id` in the `family_members` table.
    pub user_id: UserId,

    /// The family this user belongs to. Extracted from `custom:family_id`.
    ///
    /// RULE: This value is the authoritative `family_id` for all queries.
    /// Never use a `family_id` from the request body or URL parameters.
    pub family_id: FamilyId,

    /// The user's role within their family. Extracted from `custom:family_role`.
    pub role: Role,

    /// User's email address from the JWT `email` claim.
    pub email: String,
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
///     // claims.family_id is safe to use — came from JWT, not client input
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
    Forbidden(String),
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
