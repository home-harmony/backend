//! Authentication claims, Cognito Option A + Option 2B validators, and Axum extractors.
//!
//! ## Architecture (Option A + Option 2B + ADR-0011)
//!
//! 1. **Production AWS Lambda**: API Gateway HTTP API validates the Cognito RS256 signature,
//!    `iss`, `exp`, and `aud`/`client_id`. Middleware extracts claims strictly from
//!    `request_context.authorizer.jwt.claims` (never from raw unverified HTTP headers).
//! 2. **Local Dev / Integration Tests**: [`LocalJwtVerifier`] cryptographically validates
//!    `Authorization: Bearer <jwt>` using `jsonwebtoken`.
//! 3. **Token Type Policy (Option 2B)**:
//!    - `token_use == "access"`: accepted everywhere; `email` is `None`.
//!    - `token_use == "id"`: accepted only if `email_verified` is `"true"` (critical for
//!      Google federation) and `email` parses cleanly as [`Email`].
//! 4. **Multi-Family Isolation**: Family-scoped endpoints validate `X-Family-Id` against
//!    `family_members` via [`FamilyMembershipReader`] before inserting sealed [`AuthClaims`].

use std::collections::HashMap;
use std::str::FromStr;

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use lambda_http::request::RequestContext;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use domain::{
    ports::{ActiveFamilyMembership, FamilyMembershipReader},
    value_objects::{Email, FamilyId, Permission, Role, UserId},
};

use crate::errors::ApiError;

/// HTTP header carrying the caller's selected family workspace UUID.
pub const X_FAMILY_ID_HEADER: &str = "x-family-id";

/// Sealed authenticated user identity for non-family routes (`GET /profile/families`,
/// `POST /families`, `POST /families/{id}/members`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthenticatedUser {
    user_id: UserId,
    email: Option<Email>,
}

/// Sealed authenticated caller and verified family workspace context.
///
/// Fields are private and [`Deserialize`] is intentionally omitted so untrusted JSON
/// payloads can never forge tenant identity or roles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthClaims {
    user_id: UserId,
    family_id: FamilyId,
    role: Role,
    permissions: Vec<Permission>,
    email: Option<Email>,
}

/// Semantically validated Cognito JWT identity prior to family membership resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedJwtIdentity {
    user_id: UserId,
    email: Option<Email>,
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

/// Errors that can occur during authentication or family context validation.
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

    #[error("Invalid X-Family-Id header: expected UUID")]
    InvalidFamilyId,

    #[error("Insufficient permissions for role: {0}")]
    Forbidden(Role),

    #[error("User is not an active member of this family: {0}")]
    UserNotInFamily(UserId),

    #[error("Failed to verify family membership")]
    MembershipLookupFailed,
}

impl AuthenticatedUser {
    /// Returns the authenticated Cognito `sub` UUID.
    #[must_use]
    pub const fn user_id(&self) -> UserId {
        self.user_id
    }

    /// Returns the verified email address when authenticated with a verified Cognito ID token.
    #[must_use]
    pub const fn email(&self) -> Option<&Email> {
        self.email.as_ref()
    }
}

impl AuthClaims {
    /// Returns the authenticated Cognito `sub` UUID.
    #[must_use]
    pub const fn user_id(&self) -> UserId {
        self.user_id
    }

    /// Returns the verified active family workspace ID from `X-Family-Id`.
    #[must_use]
    pub const fn family_id(&self) -> FamilyId {
        self.family_id
    }

    /// Returns the caller's verified role within `family_id`.
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// Returns custom `Role::Other` permissions resolved from `family_members`.
    #[must_use]
    pub fn permissions(&self) -> &[Permission] {
        &self.permissions
    }

    /// Returns the verified email address when present in a verified ID token.
    #[must_use]
    pub const fn email(&self) -> Option<&Email> {
        self.email.as_ref()
    }

    #[must_use]
    fn from_membership(
        identity: VerifiedJwtIdentity,
        membership: ActiveFamilyMembership,
    ) -> Self {
        Self {
            user_id: identity.user_id,
            family_id: membership.family_id,
            role: membership.role,
            permissions: membership.permissions,
            email: identity.email,
        }
    }

    /// Constructs claims for isolated unit/integration tests without bypassing production sealing.
    #[cfg(test)]
    #[must_use]
    pub fn for_test(
        user_id: UserId,
        family_id: FamilyId,
        role: Role,
        permissions: Vec<Permission>,
        email: Option<Email>,
    ) -> Self {
        Self {
            user_id,
            family_id,
            role,
            permissions,
            email,
        }
    }
}

impl VerifiedJwtIdentity {
    /// Parses and semantically validates API Gateway HTTP API v2 JWT authorizer claims.
    pub fn from_claim_map(claims: &HashMap<String, String>) -> Result<Self, AuthError> {
        let sub = claims
            .get("sub")
            .ok_or_else(|| AuthError::InvalidClaims("missing sub claim".to_string()))?;
        let user_id = parse_user_id(sub)?;
        let token_use = claims
            .get("token_use")
            .map(String::as_str)
            .ok_or_else(|| AuthError::InvalidClaims("missing token_use claim".to_string()))?;

        match token_use {
            "access" => Ok(Self {
                user_id,
                email: None,
            }),
            "id" => {
                let verified = claims
                    .get("email_verified")
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"));
                if !verified {
                    return Err(AuthError::UnverifiedEmail);
                }
                let raw_email = claims
                    .get("email")
                    .ok_or_else(|| AuthError::InvalidClaims("missing email claim".to_string()))?;
                let email = Email::try_new(raw_email)
                    .map_err(|_| AuthError::InvalidClaims("invalid email claim".to_string()))?;
                Ok(Self {
                    user_id,
                    email: Some(email),
                })
            }
            _ => Err(AuthError::InvalidClaims(
                "unsupported token_use".to_string(),
            )),
        }
    }

    /// Returns the validated Cognito `sub` UUID.
    #[must_use]
    pub const fn user_id(&self) -> UserId {
        self.user_id
    }

    /// Returns the verified optional email.
    #[must_use]
    pub const fn email(&self) -> Option<&Email> {
        self.email.as_ref()
    }

    /// Converts this verified JWT identity into a sealed [`AuthenticatedUser`].
    #[must_use]
    pub fn into_authenticated_user(self) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: self.user_id,
            email: self.email,
        }
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

    /// Verifies `Authorization: Bearer <jwt>` and enforces Cognito Option 2B semantics.
    pub fn verify_headers(&self, headers: &HeaderMap) -> Result<VerifiedJwtIdentity, AuthError> {
        let token = extract_bearer_token(headers)?;
        let mut validation = Validation::new(self.algorithm);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.validate_exp = true;
        validation.validate_aud = false;

        let token_data = decode::<LocalJwtPayload>(token, &self.decoding_key, &validation)
            .map_err(|e| AuthError::InvalidClaims(format!("JWT verification failed: {e}")))?;
        let payload = token_data.claims;

        let mut claim_map = HashMap::new();
        claim_map.insert("sub".to_string(), payload.sub);
        if let Some(token_use) = payload.token_use {
            validate_local_audience(
                &token_use,
                payload.aud.as_deref(),
                payload.client_id.as_deref(),
                &self.expected_audience,
            )?;
            claim_map.insert("token_use".to_string(), token_use);
        }
        if let Some(email) = payload.email {
            claim_map.insert("email".to_string(), email);
        }
        if let Some(verified) = payload.email_verified.as_ref().and_then(json_bool_as_str) {
            claim_map.insert("email_verified".to_string(), verified);
        }

        VerifiedJwtIdentity::from_claim_map(&claim_map)
    }
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(user) = parts.extensions.get::<AuthenticatedUser>() {
            return Ok(user.clone());
        }
        extract_apigw_jwt_identity(parts).map(VerifiedJwtIdentity::into_authenticated_user)
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
            AuthError::Forbidden(_) => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            AuthError::UserNotInFamily(_) => (StatusCode::FORBIDDEN, "USER_NOT_IN_FAMILY"),
            AuthError::MembershipLookupFailed => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "MEMBERSHIP_LOOKUP_FAILED",
            ),
        };
        ApiError::problem(status, code, self.to_string()).into_response()
    }
}

/// Extracts [`VerifiedJwtIdentity`] from API Gateway v2 authorizer request context.
pub fn extract_apigw_jwt_identity(parts: &Parts) -> Result<VerifiedJwtIdentity, AuthError> {
    let Some(RequestContext::ApiGatewayV2(ctx)) = parts.extensions.get::<RequestContext>() else {
        return Err(AuthError::MissingClaims);
    };
    let claims = ctx
        .authorizer
        .as_ref()
        .and_then(|auth| auth.jwt.as_ref())
        .map(|jwt| &jwt.claims)
        .ok_or(AuthError::MissingClaims)?;

    VerifiedJwtIdentity::from_claim_map(claims)
}

/// Parses `X-Family-Id` and verifies active membership via `reader`.
pub async fn resolve_family_auth_claims<R: FamilyMembershipReader>(
    reader: &R,
    identity: VerifiedJwtIdentity,
    headers: &HeaderMap,
) -> Result<AuthClaims, AuthError> {
    let family_id = parse_family_id_header(headers)?;
    let user_id = identity.user_id();

    let membership = reader
        .find_active_membership(family_id, user_id)
        .await
        .map_err(|_| AuthError::MembershipLookupFailed)?;

    let Some(membership) = membership else {
        return Err(AuthError::UserNotInFamily(user_id));
    };

    Ok(AuthClaims::from_membership(identity, membership))
}

fn parse_user_id(raw: &str) -> Result<UserId, AuthError> {
    let uuid = Uuid::parse_str(raw.trim())
        .map_err(|_| AuthError::InvalidClaims("sub claim must be a valid UUID".to_string()))?;
    Ok(UserId::from_uuid(uuid))
}

fn parse_family_id_header(headers: &HeaderMap) -> Result<FamilyId, AuthError> {
    let Some(raw) = headers.get(X_FAMILY_ID_HEADER) else {
        return Err(AuthError::MissingFamilyId);
    };
    let text = raw.to_str().map_err(|_| AuthError::InvalidFamilyId)?.trim();
    if text.is_empty() {
        return Err(AuthError::MissingFamilyId);
    }
    FamilyId::from_str(text).map_err(|_| AuthError::InvalidFamilyId)
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AuthError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(AuthError::MissingClaims);
    };
    let text = value
        .to_str()
        .map_err(|_| AuthError::InvalidClaims("invalid Authorization header".to_string()))?;
    let token = text
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(AuthError::MissingClaims)?;
    Ok(token)
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
            "token audience/client_id mismatch".to_string(),
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

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, header},
        routing::get,
    };
    use chrono::Utc;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use lambda_http::aws_lambda_events::apigw::{
        ApiGatewayRequestAuthorizer, ApiGatewayRequestAuthorizerJwtDescription,
        ApiGatewayV2httpRequestContext,
    };
    use std::convert::Infallible;
    use tower::ServiceExt;

    #[derive(Clone, Default)]
    struct StubMembershipReader {
        found: Option<ActiveFamilyMembership>,
    }

    impl FamilyMembershipReader for StubMembershipReader {
        type Error = Infallible;

        async fn find_active_membership(
            &self,
            family_id: FamilyId,
            user_id: UserId,
        ) -> Result<Option<ActiveFamilyMembership>, Self::Error> {
            Ok(self
                .found
                .clone()
                .filter(|m| m.family_id == family_id && m.user_id == user_id))
        }
    }

    fn apigw_context_with_claims(claims: HashMap<String, String>) -> RequestContext {
        let mut jwt = ApiGatewayRequestAuthorizerJwtDescription::default();
        jwt.claims = claims;
        let mut authorizer = ApiGatewayRequestAuthorizer::default();
        authorizer.jwt = Some(jwt);
        let mut ctx = ApiGatewayV2httpRequestContext::default();
        ctx.authorizer = Some(authorizer);
        RequestContext::ApiGatewayV2(ctx)
    }

    #[test]
    fn option_2b_accepts_access_token_without_email_and_verified_id_token() {
        let sub = UserId::new_v4();
        let access_claims = HashMap::from([
            ("sub".to_string(), sub.to_string()),
            ("token_use".to_string(), "access".to_string()),
            ("email".to_string(), "ignored@example.com".to_string()),
        ]);
        let access_id = VerifiedJwtIdentity::from_claim_map(&access_claims).unwrap();
        assert_eq!(access_id.user_id(), sub);
        assert!(access_id.email().is_none());

        let id_claims = HashMap::from([
            ("sub".to_string(), sub.to_string()),
            ("token_use".to_string(), "id".to_string()),
            ("email_verified".to_string(), "true".to_string()),
            ("email".to_string(), "  Alice@Gmail.COM ".to_string()),
        ]);
        let id_identity = VerifiedJwtIdentity::from_claim_map(&id_claims).unwrap();
        assert_eq!(
            id_identity.email().map(Email::as_str),
            Some("alice@gmail.com")
        );
    }

    #[test]
    fn option_2b_rejects_unverified_id_token_and_invalid_sub_or_token_use() {
        let sub = UserId::new_v4();
        let unverified = HashMap::from([
            ("sub".to_string(), sub.to_string()),
            ("token_use".to_string(), "id".to_string()),
            ("email_verified".to_string(), "false".to_string()),
            ("email".to_string(), "alice@gmail.com".to_string()),
        ]);
        assert!(matches!(
            VerifiedJwtIdentity::from_claim_map(&unverified),
            Err(AuthError::UnverifiedEmail)
        ));

        let bad_use = HashMap::from([
            ("sub".to_string(), sub.to_string()),
            ("token_use".to_string(), "refresh".to_string()),
        ]);
        assert!(matches!(
            VerifiedJwtIdentity::from_claim_map(&bad_use),
            Err(AuthError::InvalidClaims(_))
        ));

        let bad_sub = HashMap::from([
            ("sub".to_string(), "not-a-uuid".to_string()),
            ("token_use".to_string(), "access".to_string()),
        ]);
        assert!(matches!(
            VerifiedJwtIdentity::from_claim_map(&bad_sub),
            Err(AuthError::InvalidClaims(_))
        ));
    }

    #[test]
    fn local_jwt_verifier_validates_signed_access_and_id_tokens() {
        #[derive(Serialize)]
        struct TestClaims<'a> {
            sub: String,
            iss: &'a str,
            token_use: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            client_id: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            aud: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            email: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            email_verified: Option<bool>,
            exp: usize,
        }

        let secret = b"test-signing-secret-must-be-long-enough-for-hs256";
        let verifier = LocalJwtVerifier::new(
            DecodingKey::from_secret(secret),
            Algorithm::HS256,
            "https://cognito-idp.us-east-1.amazonaws.com/pool",
            "client-123",
        );

        let sub = UserId::new_v4();
        let access_jwt = encode(
            &Header::new(Algorithm::HS256),
            &TestClaims {
                sub: sub.to_string(),
                iss: "https://cognito-idp.us-east-1.amazonaws.com/pool",
                token_use: "access",
                client_id: Some("client-123"),
                aud: None,
                email: None,
                email_verified: None,
                exp: (Utc::now().timestamp() + 300) as usize,
            },
            &EncodingKey::from_secret(secret),
        )
        .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {access_jwt}").parse().unwrap(),
        );
        let identity = verifier.verify_headers(&headers).unwrap();
        assert_eq!(identity.user_id(), sub);
        assert!(identity.email().is_none());

        let wrong_aud_jwt = encode(
            &Header::new(Algorithm::HS256),
            &TestClaims {
                sub: sub.to_string(),
                iss: "https://cognito-idp.us-east-1.amazonaws.com/pool",
                token_use: "id",
                client_id: None,
                aud: Some("other-client"),
                email: Some("alice@gmail.com"),
                email_verified: Some(true),
                exp: (Utc::now().timestamp() + 300) as usize,
            },
            &EncodingKey::from_secret(secret),
        )
        .unwrap();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {wrong_aud_jwt}").parse().unwrap(),
        );
        assert!(matches!(
            verifier.verify_headers(&headers),
            Err(AuthError::InvalidClaims(_))
        ));
    }

    #[tokio::test]
    async fn authenticated_user_extractor_succeeds_from_apigw_context_without_family_id() {
        let sub = UserId::new_v4();
        let app = Router::new().route(
            "/profile/families",
            get(|user: AuthenticatedUser| async move { user.user_id().to_string() }),
        );

        let req = Request::builder()
            .uri("/profile/families")
            .header("x-user-id", UserId::new_v4().to_string())
            .body(Body::empty())
            .unwrap();
        // Raw header alone must fail closed with 401
        let unauthed = app.clone().oneshot(req).await.unwrap();
        assert_eq!(unauthed.status(), StatusCode::UNAUTHORIZED);

        let mut req_authed = Request::builder()
            .uri("/profile/families")
            .body(Body::empty())
            .unwrap();
        req_authed.extensions_mut().insert(apigw_context_with_claims(
            HashMap::from([
                ("sub".to_string(), sub.to_string()),
                ("token_use".to_string(), "access".to_string()),
            ]),
        ));
        let ok_resp = app.oneshot(req_authed).await.unwrap();
        assert_eq!(ok_resp.status(), StatusCode::OK);
        let body = to_bytes(ok_resp.into_body(), 1024).await.unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), sub.to_string());
    }

    #[tokio::test]
    async fn resolve_family_auth_claims_enforces_header_and_membership() {
        let user_id = UserId::new_v4();
        let family_id = FamilyId::new_v4();
        let identity = VerifiedJwtIdentity::from_claim_map(&HashMap::from([
            ("sub".to_string(), user_id.to_string()),
            ("token_use".to_string(), "access".to_string()),
        ]))
        .unwrap();
        let reader = StubMembershipReader {
            found: Some(ActiveFamilyMembership {
                member_id: domain::value_objects::MemberId::new_v4(),
                family_id,
                user_id,
                role: Role::Member,
                permissions: Vec::new(),
            }),
        };

        // Missing X-Family-Id -> 400 MISSING_FAMILY_ID
        let missing = resolve_family_auth_claims(&reader, identity.clone(), &HeaderMap::new())
            .await
            .unwrap_err()
            .into_response();
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);

        // Malformed X-Family-Id -> 400 INVALID_FAMILY_ID
        let mut bad_headers = HeaderMap::new();
        bad_headers.insert(X_FAMILY_ID_HEADER, "not-a-uuid".parse().unwrap());
        let invalid = resolve_family_auth_claims(&reader, identity.clone(), &bad_headers)
            .await
            .unwrap_err()
            .into_response();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        // Non-member family -> 403 USER_NOT_IN_FAMILY
        let mut other_headers = HeaderMap::new();
        other_headers.insert(
            X_FAMILY_ID_HEADER,
            FamilyId::new_v4().to_string().parse().unwrap(),
        );
        let forbidden = resolve_family_auth_claims(&reader, identity.clone(), &other_headers)
            .await
            .unwrap_err()
            .into_response();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        // Valid active member -> sealed AuthClaims
        let mut valid_headers = HeaderMap::new();
        valid_headers.insert(X_FAMILY_ID_HEADER, family_id.to_string().parse().unwrap());
        let claims = resolve_family_auth_claims(&reader, identity, &valid_headers)
            .await
            .unwrap();
        assert_eq!(claims.user_id(), user_id);
        assert_eq!(claims.family_id(), family_id);
        assert_eq!(claims.role(), Role::Member);
    }

    #[tokio::test]
    async fn auth_error_maps_to_problem_json_with_security_headers() {
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
                AuthError::UnverifiedEmail,
                StatusCode::UNAUTHORIZED,
                "UNVERIFIED_EMAIL",
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
}
