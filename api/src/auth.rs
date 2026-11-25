//! Authentication claims, static route authorization policy, and Axum extractors (ADR-0011).
//!
//! 1. API Gateway validates the Cognito JWT signature and injects verified identity claims;
//!    claims are read only from `requestContext.authorizer.jwt.claims`, never from raw headers.
//! 2. [`UserIdentity`] represents the verified caller (`user_id` + optional `email`) under the
//!    strict Option 2B token policy: `token_use` is required; `"access"` ignores `email`;
//!    `"id"` requires `email_verified == "true"` and a valid `email`; anything else fails closed.
//!    [`LocalJwtVerifier`] applies the same policy to `Authorization: Bearer` tokens locally.
//! 3. Family-scoped routes pass through [`family_auth_middleware`], which parses `X-Family-Id`,
//!    resolves active membership via [`MembershipReader`], enforces [`FAMILY_ROUTE_POLICIES`] via
//!    [`evaluate_family_access`], and inserts [`AuthClaims`] into request extensions.

use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{
        HeaderMap, StatusCode, header,
        request::Parts,
        uri::{PathAndQuery, Uri},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use lambda_http::request::RequestContext;
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

/// Local / integration-test JWT verifier (`Authorization: Bearer <jwt>`).
#[derive(Clone)]
pub struct LocalJwtVerifier {
    decoding_key: DecodingKey,
    algorithm: Algorithm,
    issuer: String,
    expected_audience: String,
}

#[derive(Debug, Deserialize)]
struct LocalJwtPayload {
    sub: String,
    token_use: Option<String>,
    aud: Option<String>,
    client_id: Option<String>,
    email: Option<String>,
    email_verified: Option<serde_json::Value>,
}

/// Errors that can occur during authentication or family context verification.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Authentication claims not found in request context")]
    MissingClaims,

    #[error("Invalid JWT claims: {0}")]
    InvalidClaims(String),

    #[error("ID token email address is not verified")]
    UnverifiedEmail,

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
}

impl UserIdentity {
    /// Parses and validates API Gateway JWT authorizer claims under Option 2B.
    pub fn from_claim_map(claims: &HashMap<String, String>) -> Result<Self, AuthError> {
        let raw_sub = claims.get("sub").ok_or(AuthError::MissingClaims)?;
        let user_id = parse_sub_claim(raw_sub)?;
        let token_use = claims
            .get("token_use")
            .map(String::as_str)
            .ok_or_else(|| AuthError::InvalidClaims("missing token_use claim".to_owned()))?;
        let email = match token_use {
            "access" => None,
            "id" => Some(parse_verified_id_email(claims)?),
            _ => return Err(AuthError::InvalidClaims("unsupported token_use".to_owned())),
        };
        Ok(Self { user_id, email })
    }
}

impl LocalJwtVerifier {
    /// Creates a cryptographic JWT verifier for local development and tests.
    #[must_use]
    pub fn new(
        decoding_key: DecodingKey,
        algorithm: Algorithm,
        issuer: impl Into<String>,
        expected_audience: impl Into<String>,
    ) -> Self {
        Self {
            decoding_key,
            algorithm,
            issuer: issuer.into(),
            expected_audience: expected_audience.into(),
        }
    }

    /// Verifies `Authorization: Bearer <jwt>` and enforces Option 2B semantics.
    pub fn verify_headers(&self, headers: &HeaderMap) -> Result<UserIdentity, AuthError> {
        let token = extract_bearer_token(headers)?;
        let mut validation = Validation::new(self.algorithm);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.validate_exp = true;
        validation.validate_aud = false;

        let payload = decode::<LocalJwtPayload>(token, &self.decoding_key, &validation)
            .map_err(|e| AuthError::InvalidClaims(format!("JWT verification failed: {e}")))?
            .claims;
        let claim_map = self.build_claim_map(payload)?;
        UserIdentity::from_claim_map(&claim_map)
    }

    fn build_claim_map(
        &self,
        payload: LocalJwtPayload,
    ) -> Result<HashMap<String, String>, AuthError> {
        let mut claim_map = HashMap::from([("sub".to_owned(), payload.sub)]);
        if let Some(token_use) = payload.token_use {
            validate_local_audience(
                &token_use,
                payload.aud.as_deref(),
                payload.client_id.as_deref(),
                &self.expected_audience,
            )?;
            claim_map.insert("token_use".to_owned(), token_use);
        }
        if let Some(email) = payload.email {
            claim_map.insert("email".to_owned(), email);
        }
        if let Some(verified) = payload.email_verified.as_ref().and_then(json_bool_as_str) {
            claim_map.insert("email_verified".to_owned(), verified);
        }
        Ok(claim_map)
    }
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
            AuthError::UnverifiedEmail => (StatusCode::UNAUTHORIZED, "UNVERIFIED_EMAIL"),
            AuthError::MissingFamilyId => (StatusCode::BAD_REQUEST, "MISSING_FAMILY_ID"),
            AuthError::InvalidFamilyId => (StatusCode::BAD_REQUEST, "INVALID_FAMILY_ID"),
            AuthError::UserNotInFamily(_) => (StatusCode::FORBIDDEN, "USER_NOT_IN_FAMILY"),
            AuthError::FamilyIdMismatch => (StatusCode::FORBIDDEN, "FAMILY_ID_MISMATCH"),
            AuthError::InsufficientRole(_) => (StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE"),
        };
        ApiError::problem(status, code, self.to_string()).into_response()
    }
}

/// Extracts [`UserIdentity`] from API Gateway HTTP API (v2) Cognito JWT authorizer claims.
pub fn extract_user_identity_from_context(ctx: &RequestContext) -> Result<UserIdentity, AuthError> {
    let RequestContext::ApiGatewayV2(v2) = ctx else {
        return Err(AuthError::MissingClaims);
    };
    let Some(jwt) = v2.authorizer.as_ref().and_then(|a| a.jwt.as_ref()) else {
        return Err(AuthError::MissingClaims);
    };
    UserIdentity::from_claim_map(&jwt.claims)
}

/// Axum middleware populating [`UserIdentity`] from [`RequestContext`] when not pre-injected.
pub async fn lambda_identity_middleware(mut req: Request, next: Next) -> Response {
    if req.extensions().get::<UserIdentity>().is_some() {
        return next.run(req).await;
    }
    let Some(ctx) = req.extensions().get::<RequestContext>() else {
        return AuthError::MissingClaims.into_response();
    };
    match extract_user_identity_from_context(ctx) {
        Ok(identity) => {
            req.extensions_mut().insert(identity);
            next.run(req).await
        }
        Err(err) => err.into_response(),
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

/// Strips an API Gateway HTTP API v2 named stage prefix (e.g. `/dev`) before Axum routing,
/// preserving query strings and leaving `$default` or non-matching paths untouched.
pub fn strip_apigw_stage_prefix<B>(mut req: axum::http::Request<B>) -> axum::http::Request<B> {
    let Some(stage) = extract_named_apigw_stage(req.extensions().get::<RequestContext>()) else {
        return req;
    };
    let Some(stripped_path) = strip_stage_segment(req.uri().path(), stage) else {
        return req;
    };
    if let Some(new_uri) = rebuild_uri_with_path(req.uri(), &stripped_path) {
        *req.uri_mut() = new_uri;
    }
    req
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

fn parse_sub_claim(raw_sub: &str) -> Result<UserId, AuthError> {
    if raw_sub.trim().is_empty() {
        return Err(AuthError::InvalidClaims(
            "JWT 'sub' claim must not be empty".to_owned(),
        ));
    }
    Uuid::parse_str(raw_sub)
        .map(UserId::from_uuid)
        .map_err(|err| AuthError::InvalidClaims(format!("Invalid JWT 'sub' UUID: {err}")))
}

fn parse_verified_id_email(claims: &HashMap<String, String>) -> Result<Email, AuthError> {
    let verified = claims
        .get("email_verified")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if !verified {
        return Err(AuthError::UnverifiedEmail);
    }
    let raw_email = claims
        .get("email")
        .ok_or_else(|| AuthError::InvalidClaims("missing email claim".to_owned()))?;
    Email::try_new(raw_email.as_str())
        .map_err(|err| AuthError::InvalidClaims(format!("Invalid JWT 'email' claim: {err}")))
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AuthError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(AuthError::MissingClaims);
    };
    let text = value
        .to_str()
        .map_err(|_| AuthError::InvalidClaims("invalid Authorization header".to_owned()))?;
    text.strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or(AuthError::MissingClaims)
}

fn validate_local_audience(
    token_use: &str,
    aud: Option<&str>,
    client_id: Option<&str>,
    expected: &str,
) -> Result<(), AuthError> {
    let matches = match token_use {
        "access" => client_id == Some(expected) || aud == Some(expected),
        "id" => aud == Some(expected),
        _ => false,
    };
    if !matches {
        return Err(AuthError::InvalidClaims(
            "token audience/client_id mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn json_bool_as_str(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn extract_named_apigw_stage(ctx: Option<&RequestContext>) -> Option<&str> {
    let RequestContext::ApiGatewayV2(v2) = ctx? else {
        return None;
    };
    let stage = v2.stage.as_deref()?;
    if stage.is_empty() || stage == "$default" {
        return None;
    }
    Some(stage)
}

fn strip_stage_segment(path: &str, stage: &str) -> Option<String> {
    let prefix = format!("/{stage}");
    if path == prefix {
        return Some("/".to_owned());
    }
    let rest = path.strip_prefix(&format!("{prefix}/"))?;
    Some(format!("/{rest}"))
}

fn rebuild_uri_with_path(uri: &Uri, new_path: &str) -> Option<Uri> {
    let path_and_query = match uri.query() {
        Some(query) => format!("{new_path}?{query}"),
        None => new_path.to_owned(),
    };
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(PathAndQuery::try_from(path_and_query).ok()?);
    Uri::from_parts(parts).ok()
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use jsonwebtoken::{EncodingKey, Header, encode};

    use axum::{
        Router,
        body::{Body, to_bytes},
        http::header,
        middleware,
        routing::get,
    };
    use lambda_http::aws_lambda_events::apigw::{
        ApiGatewayProxyRequestContext, ApiGatewayRequestAuthorizer,
        ApiGatewayRequestAuthorizerJwtDescription, ApiGatewayV2httpRequestContext,
    };
    use tower::ServiceExt;

    use super::*;

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
        for (err, expected_status, expected_code) in auth_error_cases() {
            let resp = err.into_response();
            assert_eq!(resp.status(), expected_status);
            assert_eq!(
                resp.headers().get(header::CONTENT_TYPE).unwrap(),
                "application/problem+json"
            );
            assert_eq!(
                resp.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
                "nosniff"
            );
            assert_eq!(
                resp.headers().get(header::CACHE_CONTROL).unwrap(),
                "no-store"
            );
            let bytes = to_bytes(resp.into_body(), 16 * 1024).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["code"], expected_code);
            assert_eq!(body["status"], expected_status.as_u16());
        }
    }

    #[test]
    fn extract_user_identity_supports_valid_sub_with_and_without_email() {
        let uid = UserId::new_v4();
        let with_email = v2_context_with_claims(HashMap::from([
            ("sub".to_owned(), uid.to_string()),
            ("token_use".to_owned(), "id".to_owned()),
            ("email_verified".to_owned(), "true".to_owned()),
            ("email".to_owned(), "  Alice@Example.COM  ".to_owned()),
        ]));
        let id = extract_user_identity_from_context(&with_email).unwrap();
        assert_eq!(id.user_id, uid);
        assert_eq!(
            id.email.as_ref().map(Email::as_str),
            Some("alice@example.com")
        );

        let without_email = v2_context_with_claims(access_claims(uid));
        let id_no_email = extract_user_identity_from_context(&without_email).unwrap();
        assert_eq!(id_no_email.user_id, uid);
        assert!(id_no_email.email.is_none());
    }

    #[test]
    fn extract_user_identity_rejects_missing_context_or_sub() {
        let v1 = RequestContext::ApiGatewayV1(ApiGatewayProxyRequestContext::default());
        assert!(matches!(
            extract_user_identity_from_context(&v1),
            Err(AuthError::MissingClaims)
        ));

        let no_auth = RequestContext::ApiGatewayV2(ApiGatewayV2httpRequestContext::default());
        assert!(matches!(
            extract_user_identity_from_context(&no_auth),
            Err(AuthError::MissingClaims)
        ));

        let missing_sub = v2_context_with_claims(HashMap::new());
        assert!(matches!(
            extract_user_identity_from_context(&missing_sub),
            Err(AuthError::MissingClaims)
        ));
    }

    #[test]
    fn extract_user_identity_rejects_malformed_sub_or_email() {
        let empty_sub = v2_context_with_claims(HashMap::from([
            ("sub".to_owned(), "   ".to_owned()),
            ("token_use".to_owned(), "access".to_owned()),
        ]));
        assert!(matches!(
            extract_user_identity_from_context(&empty_sub),
            Err(AuthError::InvalidClaims(_))
        ));

        let bad_sub = v2_context_with_claims(HashMap::from([
            ("sub".to_owned(), "not-a-uuid".to_owned()),
            ("token_use".to_owned(), "access".to_owned()),
        ]));
        assert!(matches!(
            extract_user_identity_from_context(&bad_sub),
            Err(AuthError::InvalidClaims(_))
        ));

        let bad_email = v2_context_with_claims(HashMap::from([
            ("sub".to_owned(), UserId::new_v4().to_string()),
            ("token_use".to_owned(), "id".to_owned()),
            ("email_verified".to_owned(), "true".to_owned()),
            ("email".to_owned(), "bad-email".to_owned()),
        ]));
        assert!(matches!(
            extract_user_identity_from_context(&bad_email),
            Err(AuthError::InvalidClaims(_))
        ));
    }

    #[test]
    fn option_2b_access_token_ignores_email_claim() {
        let uid = UserId::new_v4();
        let mut claims = access_claims(uid);
        claims.insert("email".to_owned(), "ignored@example.com".to_owned());
        let identity = UserIdentity::from_claim_map(&claims).unwrap();
        assert_eq!(identity.user_id, uid);
        assert!(identity.email.is_none());
    }

    #[test]
    fn option_2b_rejects_unverified_id_token() {
        let uid = UserId::new_v4();
        for verified in [Some("false"), None] {
            let mut claims = HashMap::from([
                ("sub".to_owned(), uid.to_string()),
                ("token_use".to_owned(), "id".to_owned()),
                ("email".to_owned(), "alice@gmail.com".to_owned()),
            ]);
            if let Some(v) = verified {
                claims.insert("email_verified".to_owned(), v.to_owned());
            }
            assert!(matches!(
                UserIdentity::from_claim_map(&claims),
                Err(AuthError::UnverifiedEmail)
            ));
        }
    }

    #[test]
    fn option_2b_fails_closed_on_missing_or_unsupported_token_use() {
        let uid = UserId::new_v4();
        let missing = HashMap::from([("sub".to_owned(), uid.to_string())]);
        assert!(matches!(
            UserIdentity::from_claim_map(&missing),
            Err(AuthError::InvalidClaims(_))
        ));

        let mut refresh = access_claims(uid);
        refresh.insert("token_use".to_owned(), "refresh".to_owned());
        assert!(matches!(
            UserIdentity::from_claim_map(&refresh),
            Err(AuthError::InvalidClaims(_))
        ));
    }

    #[test]
    fn local_jwt_verifier_validates_signed_access_and_id_tokens() {
        let secret = b"test-signing-secret-must-be-long-enough-for-hs256";
        let issuer = "https://cognito-idp.us-east-1.amazonaws.com/pool";
        let verifier = LocalJwtVerifier::new(
            DecodingKey::from_secret(secret),
            Algorithm::HS256,
            issuer,
            "client-123",
        );
        let sub = UserId::new_v4();

        let access = signed_jwt(secret, issuer, sub, "access", Some("client-123"), None);
        let identity = verifier.verify_headers(&bearer(&access)).unwrap();
        assert_eq!(identity.user_id, sub);
        assert!(identity.email.is_none());

        let wrong_aud = signed_jwt(secret, issuer, sub, "id", None, Some("other-client"));
        assert!(matches!(
            verifier.verify_headers(&bearer(&wrong_aud)),
            Err(AuthError::InvalidClaims(_))
        ));
    }

    #[test]
    fn local_jwt_verifier_rejects_missing_authorization_header() {
        let verifier = LocalJwtVerifier::new(
            DecodingKey::from_secret(b"secret"),
            Algorithm::HS256,
            "iss",
            "client",
        );
        assert!(matches!(
            verifier.verify_headers(&HeaderMap::new()),
            Err(AuthError::MissingClaims)
        ));
    }

    #[tokio::test]
    async fn lambda_identity_middleware_handles_passthrough_context_and_missing() {
        let app = whoami_app();

        let preset_id = UserId::new_v4();
        let (s1, b1) = send_identity_request(
            app.clone(),
            Some(UserIdentity {
                user_id: preset_id,
                email: None,
            }),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            (s1, b1.as_slice()),
            (StatusCode::OK, preset_id.to_string().as_bytes())
        );

        let ctx_id = UserId::new_v4();
        let ctx = v2_context_with_claims(access_claims(ctx_id));
        let (s2, b2) = send_identity_request(app.clone(), None, Some(ctx))
            .await
            .unwrap();
        assert_eq!(
            (s2, b2.as_slice()),
            (StatusCode::OK, ctx_id.to_string().as_bytes())
        );

        let (s3, _) = send_identity_request(app, None, None).await.unwrap();
        assert_eq!(s3, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn strip_apigw_stage_prefix_strips_named_stage_and_preserves_query()
    -> Result<(), axum::http::Error> {
        let cases = [
            ("/dev/families/123", Some("dev"), "/families/123"),
            (
                "/dev/profile/families?cursor=abc&limit=10",
                Some("dev"),
                "/profile/families?cursor=abc&limit=10",
            ),
            ("/dev", Some("dev"), "/"),
        ];
        for (input, stage, expected) in cases {
            let stripped = strip_apigw_stage_prefix(request_with_stage(input, stage)?);
            assert_eq!(stripped.uri().to_string(), expected);
        }
        Ok(())
    }

    #[test]
    fn strip_apigw_stage_prefix_ignores_partial_matches_default_and_missing_stage()
    -> Result<(), axum::http::Error> {
        let cases = [
            ("/developer/foo", Some("dev")),
            ("/families/123", Some("dev")),
            ("/dev/families/123", Some("$default")),
            ("/dev/families/123", None),
        ];
        for (input, stage) in cases {
            let unchanged = strip_apigw_stage_prefix(request_with_stage(input, stage)?);
            assert_eq!(unchanged.uri().to_string(), input);
        }
        Ok(())
    }

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

    fn auth_error_cases() -> [(AuthError, StatusCode, &'static str); 8] {
        use AuthError as E;
        use StatusCode as S;
        [
            (E::MissingClaims, S::UNAUTHORIZED, "MISSING_CLAIMS"),
            (
                E::InvalidClaims("bad token".to_owned()),
                S::UNAUTHORIZED,
                "INVALID_CLAIMS",
            ),
            (E::UnverifiedEmail, S::UNAUTHORIZED, "UNVERIFIED_EMAIL"),
            (E::MissingFamilyId, S::BAD_REQUEST, "MISSING_FAMILY_ID"),
            (E::InvalidFamilyId, S::BAD_REQUEST, "INVALID_FAMILY_ID"),
            (
                E::UserNotInFamily(UserId::new_v4()),
                S::FORBIDDEN,
                "USER_NOT_IN_FAMILY",
            ),
            (E::FamilyIdMismatch, S::FORBIDDEN, "FAMILY_ID_MISMATCH"),
            (
                E::InsufficientRole(Role::Child),
                S::FORBIDDEN,
                "INSUFFICIENT_ROLE",
            ),
        ]
    }

    fn whoami_app() -> Router {
        async fn whoami(identity: UserIdentity) -> String {
            identity.user_id.to_string()
        }
        Router::new()
            .route("/whoami", get(whoami))
            .layer(middleware::from_fn(lambda_identity_middleware))
    }

    async fn send_identity_request(
        app: Router,
        preset: Option<UserIdentity>,
        ctx: Option<RequestContext>,
    ) -> Result<(StatusCode, Vec<u8>), String> {
        let mut req = Request::builder()
            .uri("/whoami")
            .body(Body::empty())
            .map_err(|err| err.to_string())?;
        if let Some(identity) = preset {
            req.extensions_mut().insert(identity);
        }
        if let Some(context) = ctx {
            req.extensions_mut().insert(context);
        }
        let Ok(resp) = app.oneshot(req).await;
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1024)
            .await
            .map_err(|err| err.to_string())?;
        Ok((status, bytes.to_vec()))
    }

    fn access_claims(uid: UserId) -> HashMap<String, String> {
        HashMap::from([
            ("sub".to_owned(), uid.to_string()),
            ("token_use".to_owned(), "access".to_owned()),
        ])
    }

    fn bearer(jwt: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {jwt}").parse().unwrap(),
        );
        headers
    }

    fn signed_jwt(
        secret: &[u8],
        iss: &str,
        sub: UserId,
        token_use: &str,
        client_id: Option<&str>,
        aud: Option<&str>,
    ) -> String {
        let exp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 300;
        let mut claims = serde_json::json!({
            "sub": sub.to_string(), "iss": iss, "token_use": token_use, "exp": exp,
        });
        if let Some(c) = client_id {
            claims["client_id"] = c.into();
        }
        if let Some(a) = aud {
            claims["aud"] = a.into();
            claims["email"] = "alice@gmail.com".into();
            claims["email_verified"] = true.into();
        }
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret),
        )
        .unwrap()
    }

    fn v2_context_with_claims(claims: HashMap<String, String>) -> RequestContext {
        let mut jwt = ApiGatewayRequestAuthorizerJwtDescription::default();
        jwt.claims = claims;
        let mut authorizer = ApiGatewayRequestAuthorizer::default();
        authorizer.jwt = Some(jwt);
        let mut v2 = ApiGatewayV2httpRequestContext::default();
        v2.authorizer = Some(authorizer);
        RequestContext::ApiGatewayV2(v2)
    }

    fn request_with_stage(
        uri: &str,
        stage: Option<&str>,
    ) -> Result<axum::http::Request<Body>, axum::http::Error> {
        let mut req = axum::http::Request::builder()
            .uri(uri)
            .body(Body::empty())?;
        let mut v2 = ApiGatewayV2httpRequestContext::default();
        v2.stage = stage.map(ToOwned::to_owned);
        req.extensions_mut()
            .insert(RequestContext::ApiGatewayV2(v2));
        Ok(req)
    }
}
