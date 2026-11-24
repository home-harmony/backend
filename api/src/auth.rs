//! Authentication claims, static route authorization policy, and Axum extractors (ADR-0011).
//!
//! 1. API Gateway validates the Cognito JWT signature and injects verified identity claims.
//! 2. [`UserIdentity`] represents the verified caller (`user_id` + optional `email`).
//! 3. Family-scoped routes pass through [`family_auth_middleware`], which parses `X-Family-Id`,
//!    resolves active membership via [`MembershipReader`], enforces [`FAMILY_ROUTE_POLICIES`] via
//!    [`evaluate_family_access`], and inserts [`AuthClaims`] into request extensions.

use std::sync::Arc;

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use domain::{
    ports::{MemberAccess, MembershipReader},
    value_objects::{DomainAction, Email, FamilyId, Permission, Role, UserId},
};

use crate::errors::{ApiError, domain_error_response};

/// Canonical HTTP header selecting the active family workspace (`ADR-0011`).
pub const FAMILY_ID_HEADER: &str = "x-family-id";

/// Verified caller identity extracted from API Gateway's Cognito JWT authorizer context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserIdentity {
    pub user_id: UserId,
    #[serde(default)]
    pub email: Option<Email>,
}

/// Authenticated caller and verified family workspace context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
    pub user_id: UserId,
    pub family_id: FamilyId,
    pub role: Role,
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub email: Option<Email>,
}

/// Scope and authorization requirement for a route in [`FAMILY_ROUTE_POLICIES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteScope {
    IdentityOnly,
    FamilyScoped { required_action: DomainAction },
}

/// Declarative route policy row binding HTTP `(method, path)` to [`RouteScope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutePolicy {
    pub method: &'static str,
    pub path: &'static str,
    pub scope: RouteScope,
}

/// Static policy table governing all Identity & Family routes (`AU-01`..`AU-08`).
pub const FAMILY_ROUTE_POLICIES: &[RoutePolicy] = &[
    RoutePolicy {
        method: "GET",
        path: "/profile/families",
        scope: RouteScope::IdentityOnly,
    },
    RoutePolicy {
        method: "POST",
        path: "/families",
        scope: RouteScope::IdentityOnly,
    },
    RoutePolicy {
        method: "POST",
        path: "/families/{id}/members",
        scope: RouteScope::IdentityOnly,
    },
    RoutePolicy {
        method: "GET",
        path: "/families/{id}",
        scope: RouteScope::FamilyScoped {
            required_action: DomainAction::ViewFamilyWorkspace,
        },
    },
    RoutePolicy {
        method: "GET",
        path: "/families/{id}/members",
        scope: RouteScope::FamilyScoped {
            required_action: DomainAction::ViewFamilyWorkspace,
        },
    },
    RoutePolicy {
        method: "PUT",
        path: "/families/{id}",
        scope: RouteScope::FamilyScoped {
            required_action: DomainAction::ManageFamily,
        },
    },
    RoutePolicy {
        method: "POST",
        path: "/families/{id}/invites",
        scope: RouteScope::FamilyScoped {
            required_action: DomainAction::ManageFamily,
        },
    },
    RoutePolicy {
        method: "PATCH",
        path: "/families/{id}/members/{mid}/role",
        scope: RouteScope::FamilyScoped {
            required_action: DomainAction::ManageFamily,
        },
    },
    RoutePolicy {
        method: "DELETE",
        path: "/families/{id}/members/{mid}",
        scope: RouteScope::FamilyScoped {
            required_action: DomainAction::ManageFamily,
        },
    },
];

/// Parsed state of the `X-Family-Id` HTTP header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyHeaderState {
    Missing,
    Malformed,
    Valid(FamilyId),
}

/// State injected into [`family_auth_middleware`].
pub struct FamilyAuthState<R> {
    pub membership_reader: Arc<R>,
    pub required_action: DomainAction,
}

/// Errors that can occur during authentication or family context verification.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Authentication claims not found in request context")]
    MissingClaims,

    #[error("Invalid JWT claims: {0}")]
    InvalidClaims(String),

    #[error("Missing required X-Family-Id header")]
    MissingFamilyId,

    #[error("Invalid X-Family-Id header; expected UUID")]
    InvalidFamilyId,

    #[error("User {0} is not an active member of the requested family")]
    UserNotInFamily(UserId),

    #[error("Route family ID does not match X-Family-Id header")]
    FamilyIdMismatch,

    #[error("Role '{0}' is not permitted to perform this action")]
    InsufficientRole(Role),

    #[error("Insufficient permissions for role: {0}")]
    Forbidden(Role),
}

impl<R> Clone for FamilyAuthState<R> {
    fn clone(&self) -> Self {
        Self {
            membership_reader: Arc::clone(&self.membership_reader),
            required_action: self.required_action,
        }
    }
}

impl<S> FromRequestParts<S> for UserIdentity
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<UserIdentity>()
            .cloned()
            .ok_or(AuthError::MissingClaims)
    }
}

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

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AuthError::MissingClaims => (StatusCode::UNAUTHORIZED, "MISSING_CLAIMS"),
            AuthError::InvalidClaims(_) => (StatusCode::UNAUTHORIZED, "INVALID_CLAIMS"),
            AuthError::MissingFamilyId => (StatusCode::BAD_REQUEST, "MISSING_FAMILY_ID"),
            AuthError::InvalidFamilyId => (StatusCode::BAD_REQUEST, "INVALID_FAMILY_ID"),
            AuthError::UserNotInFamily(_) => (StatusCode::FORBIDDEN, "USER_NOT_IN_FAMILY"),
            AuthError::FamilyIdMismatch => (StatusCode::FORBIDDEN, "FAMILY_ID_MISMATCH"),
            AuthError::InsufficientRole(_) => (StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE"),
            AuthError::Forbidden(_) => (StatusCode::FORBIDDEN, "FORBIDDEN"),
        };
        ApiError::problem(status, code, self.to_string()).into_response()
    }
}

/// Parses the `X-Family-Id` header into [`FamilyHeaderState`].
pub fn parse_family_id_header(headers: &HeaderMap) -> FamilyHeaderState {
    let Some(raw) = headers.get(FAMILY_ID_HEADER) else {
        return FamilyHeaderState::Missing;
    };
    let Ok(value) = raw.to_str() else {
        return FamilyHeaderState::Malformed;
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return FamilyHeaderState::Missing;
    }
    match Uuid::parse_str(trimmed) {
        Ok(id) => FamilyHeaderState::Valid(FamilyId::from_uuid(id)),
        Err(_) => FamilyHeaderState::Malformed,
    }
}

/// Extracts the `{id}` UUID segment from `/families/{id}...` routes, if present.
pub fn extract_path_family_id(path: &str) -> Option<FamilyId> {
    let rest = path.strip_prefix("/families/")?;
    let raw_id = rest.split('/').next()?;
    Uuid::parse_str(raw_id).ok().map(FamilyId::from_uuid)
}

/// Pure authorization decision table for family-scoped requests (`AU-01`..`AU-08`).
pub fn evaluate_family_access(
    identity: Option<&UserIdentity>,
    header: FamilyHeaderState,
    membership: Option<&MemberAccess>,
    path_family_id: Option<FamilyId>,
    required_action: DomainAction,
) -> Result<AuthClaims, AuthError> {
    let Some(identity) = identity else {
        return Err(AuthError::MissingClaims);
    };
    let family_id = match header {
        FamilyHeaderState::Missing => return Err(AuthError::MissingFamilyId),
        FamilyHeaderState::Malformed => return Err(AuthError::InvalidFamilyId),
        FamilyHeaderState::Valid(id) => id,
    };
    let Some(access) = membership else {
        return Err(AuthError::UserNotInFamily(identity.user_id));
    };
    if path_family_id.is_some_and(|path_id| path_id != family_id) {
        return Err(AuthError::FamilyIdMismatch);
    }
    if !access
        .role
        .can_perform(&access.permissions, required_action)
    {
        return Err(AuthError::InsufficientRole(access.role));
    }
    Ok(AuthClaims {
        user_id: identity.user_id,
        family_id,
        role: access.role,
        permissions: access.permissions.clone(),
        email: identity.email.clone(),
    })
}

/// Axum middleware enforcing `UserIdentity`, `X-Family-Id`, active membership, path match, and RBAC.
pub async fn family_auth_middleware<R>(
    State(state): State<FamilyAuthState<R>>,
    mut req: Request,
    next: Next,
) -> Response
where
    R: MembershipReader + Send + Sync + 'static,
{
    let identity = req.extensions().get::<UserIdentity>().cloned();
    let header = parse_family_id_header(req.headers());
    let path_family_id = extract_path_family_id(req.uri().path());
    let membership =
        match resolve_membership(&*state.membership_reader, identity.as_ref(), header).await {
            Ok(access) => access,
            Err(err) => return domain_error_response(err),
        };
    match evaluate_family_access(
        identity.as_ref(),
        header,
        membership.as_ref(),
        path_family_id,
        state.required_action,
    ) {
        Ok(claims) => {
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(err) => err.into_response(),
    }
}

async fn resolve_membership<R: MembershipReader>(
    reader: &R,
    identity: Option<&UserIdentity>,
    header: FamilyHeaderState,
) -> Result<Option<MemberAccess>, domain::errors::DomainError> {
    let (Some(identity), FamilyHeaderState::Valid(family_id)) = (identity, header) else {
        return Ok(None);
    };
    reader
        .find_active_membership(family_id, identity.user_id)
        .await
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
                AuthError::MissingFamilyId,
                StatusCode::BAD_REQUEST,
                "MISSING_FAMILY_ID",
            ),
            (
                AuthError::InvalidFamilyId,
                StatusCode::BAD_REQUEST,
                "INVALID_FAMILY_ID",
            ),
            (
                AuthError::UserNotInFamily(UserId::new_v4()),
                StatusCode::FORBIDDEN,
                "USER_NOT_IN_FAMILY",
            ),
            (
                AuthError::FamilyIdMismatch,
                StatusCode::FORBIDDEN,
                "FAMILY_ID_MISMATCH",
            ),
            (
                AuthError::InsufficientRole(Role::Child),
                StatusCode::FORBIDDEN,
                "INSUFFICIENT_ROLE",
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
