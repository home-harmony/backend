//! Blind static-table conformance tests for `backend/specs/api/multi-family-auth.md` (`AU-01`..`AU-12`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
    middleware,
    routing::{MethodRouter, delete, get, patch, post, put},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, encode};
use serde::Serialize;
use tower::ServiceExt;

use api::{
    auth::{
        ACCOUNTS_ROUTE_POLICIES, AuthClaims, FAMILY_ID_HEADER, FAMILY_ROUTE_POLICIES,
        FamilyAuthState, LEDGER_ROUTE_POLICIES, LocalJwtVerifier, RoutePolicy, RouteScope,
        UserIdentity, family_auth_middleware,
    },
    response::{no_store_cache_layer, nosniff_layer},
};
use axum::response::IntoResponse;
use domain::{
    errors::DomainError,
    ports::{MemberAccess, MembershipReader},
    value_objects::{FamilyId, MemberId, Permission, Role, UserId},
};

#[derive(Clone, Copy)]
enum TargetFamily {
    FamilyA,
    FamilyB,
}

#[derive(Clone, Copy)]
enum HeaderSpec {
    Absent,
    Empty,
    Malformed,
    FamilyA,
    FamilyB,
}

#[derive(Clone, Copy)]
enum RouteTarget {
    ProfileFamilies,
    CreateFamily,
    AcceptInvite(TargetFamily),
    GetFamily(TargetFamily),
    GetMembers(TargetFamily),
    UpdateFamily(TargetFamily),
    CreateInvite(TargetFamily),
    ChangeMemberRole(TargetFamily),
    RemoveMember(TargetFamily),
}

#[derive(Clone, Copy)]
enum ExpectedHttp {
    Status(StatusCode),
    Problem(StatusCode, &'static str),
}

struct AuthRuleCase {
    id: &'static str,
    has_identity: bool,
    membership: Option<(TargetFamily, Role)>,
    permissions: &'static [Permission],
    header: HeaderSpec,
    route: RouteTarget,
    expected: ExpectedHttp,
}

#[derive(Default)]
struct FakeMembershipReader {
    memberships: BTreeMap<(FamilyId, UserId), MemberAccess>,
}

const ALL_EIGHT_PERMS: &[Permission] = &[
    Permission::RecordOwnTransactions,
    Permission::RecordAnyTransaction,
    Permission::ViewAllTransactions,
    Permission::ViewAllAccounts,
    Permission::ManageOwnAccounts,
    Permission::ViewLoans,
    Permission::ViewBudget,
    Permission::ViewReports,
];

const AUTH_RULE_TABLE: &[AuthRuleCase] = &[
    AuthRuleCase {
        id: "AU-01",
        has_identity: false,
        membership: None,
        permissions: &[],
        header: HeaderSpec::Absent,
        route: RouteTarget::ProfileFamilies,
        expected: ExpectedHttp::Problem(StatusCode::UNAUTHORIZED, "MISSING_CLAIMS"),
    },
    AuthRuleCase {
        id: "AU-01",
        has_identity: false,
        membership: None,
        permissions: &[],
        header: HeaderSpec::Absent,
        route: RouteTarget::CreateFamily,
        expected: ExpectedHttp::Problem(StatusCode::UNAUTHORIZED, "MISSING_CLAIMS"),
    },
    AuthRuleCase {
        id: "AU-01",
        has_identity: false,
        membership: None,
        permissions: &[],
        header: HeaderSpec::FamilyA,
        route: RouteTarget::GetFamily(TargetFamily::FamilyA),
        expected: ExpectedHttp::Problem(StatusCode::UNAUTHORIZED, "MISSING_CLAIMS"),
    },
    AuthRuleCase {
        id: "AU-02",
        has_identity: true,
        membership: None,
        permissions: &[],
        header: HeaderSpec::Absent,
        route: RouteTarget::ProfileFamilies,
        expected: ExpectedHttp::Status(StatusCode::OK),
    },
    AuthRuleCase {
        id: "AU-02",
        has_identity: true,
        membership: None,
        permissions: &[],
        header: HeaderSpec::Absent,
        route: RouteTarget::CreateFamily,
        expected: ExpectedHttp::Status(StatusCode::CREATED),
    },
    AuthRuleCase {
        id: "AU-02",
        has_identity: true,
        membership: None,
        permissions: &[],
        header: HeaderSpec::Absent,
        route: RouteTarget::AcceptInvite(TargetFamily::FamilyA),
        expected: ExpectedHttp::Status(StatusCode::CREATED),
    },
    AuthRuleCase {
        id: "AU-03",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Owner)),
        permissions: &[],
        header: HeaderSpec::Absent,
        route: RouteTarget::GetFamily(TargetFamily::FamilyA),
        expected: ExpectedHttp::Problem(StatusCode::BAD_REQUEST, "MISSING_FAMILY_ID"),
    },
    AuthRuleCase {
        id: "AU-03",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Owner)),
        permissions: &[],
        header: HeaderSpec::Empty,
        route: RouteTarget::GetFamily(TargetFamily::FamilyA),
        expected: ExpectedHttp::Problem(StatusCode::BAD_REQUEST, "MISSING_FAMILY_ID"),
    },
    AuthRuleCase {
        id: "AU-03",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Owner)),
        permissions: &[],
        header: HeaderSpec::Malformed,
        route: RouteTarget::GetFamily(TargetFamily::FamilyA),
        expected: ExpectedHttp::Problem(StatusCode::BAD_REQUEST, "INVALID_FAMILY_ID"),
    },
    AuthRuleCase {
        id: "AU-04",
        has_identity: true,
        membership: None,
        permissions: &[],
        header: HeaderSpec::FamilyA,
        route: RouteTarget::GetFamily(TargetFamily::FamilyA),
        expected: ExpectedHttp::Problem(StatusCode::FORBIDDEN, "USER_NOT_IN_FAMILY"),
    },
    AuthRuleCase {
        id: "AU-05",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Owner)),
        permissions: &[],
        header: HeaderSpec::FamilyA,
        route: RouteTarget::GetFamily(TargetFamily::FamilyB),
        expected: ExpectedHttp::Problem(StatusCode::FORBIDDEN, "FAMILY_ID_MISMATCH"),
    },
    AuthRuleCase {
        id: "AU-07",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Owner)),
        permissions: &[],
        header: HeaderSpec::FamilyA,
        route: RouteTarget::GetFamily(TargetFamily::FamilyA),
        expected: ExpectedHttp::Status(StatusCode::OK),
    },
    AuthRuleCase {
        id: "AU-07",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Member)),
        permissions: &[],
        header: HeaderSpec::FamilyA,
        route: RouteTarget::GetMembers(TargetFamily::FamilyA),
        expected: ExpectedHttp::Status(StatusCode::OK),
    },
    AuthRuleCase {
        id: "AU-07",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Child)),
        permissions: &[],
        header: HeaderSpec::FamilyA,
        route: RouteTarget::GetFamily(TargetFamily::FamilyA),
        expected: ExpectedHttp::Status(StatusCode::OK),
    },
    AuthRuleCase {
        id: "AU-07",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Guest)),
        permissions: &[],
        header: HeaderSpec::FamilyA,
        route: RouteTarget::GetMembers(TargetFamily::FamilyA),
        expected: ExpectedHttp::Status(StatusCode::OK),
    },
    AuthRuleCase {
        id: "AU-08",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Member)),
        permissions: &[],
        header: HeaderSpec::FamilyA,
        route: RouteTarget::UpdateFamily(TargetFamily::FamilyA),
        expected: ExpectedHttp::Problem(StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE"),
    },
    AuthRuleCase {
        id: "AU-08",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Child)),
        permissions: &[],
        header: HeaderSpec::FamilyA,
        route: RouteTarget::CreateInvite(TargetFamily::FamilyA),
        expected: ExpectedHttp::Problem(StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE"),
    },
    AuthRuleCase {
        id: "AU-08",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Guest)),
        permissions: ALL_EIGHT_PERMS,
        header: HeaderSpec::FamilyA,
        route: RouteTarget::ChangeMemberRole(TargetFamily::FamilyA),
        expected: ExpectedHttp::Problem(StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE"),
    },
    AuthRuleCase {
        id: "AU-08",
        has_identity: true,
        membership: Some((TargetFamily::FamilyA, Role::Guest)),
        permissions: ALL_EIGHT_PERMS,
        header: HeaderSpec::FamilyA,
        route: RouteTarget::RemoveMember(TargetFamily::FamilyA),
        expected: ExpectedHttp::Problem(StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE"),
    },
];

impl MembershipReader for FakeMembershipReader {
    async fn find_active_membership(
        &self,
        family_id: FamilyId,
        user_id: UserId,
    ) -> Result<Option<MemberAccess>, DomainError> {
        Ok(self.memberships.get(&(family_id, user_id)).cloned())
    }
}

#[tokio::test]
async fn au_auth_rule_table_conforms() {
    for case in AUTH_RULE_TABLE {
        run_auth_case(case).await.unwrap();
    }
}

#[tokio::test]
async fn au_06_multi_family_caller_switches_workspace_cleanly_via_header() {
    let (user_id, family_a, family_b, app) = multi_family_test_app();
    let identity = Some(UserIdentity {
        user_id,
        email: None,
    });

    let req_a = build_request(
        RouteTarget::CreateInvite(TargetFamily::FamilyA),
        HeaderSpec::FamilyA,
        identity.clone(),
        family_a,
        family_b,
    )
    .unwrap();
    let resp_a = app.clone().oneshot(req_a).await.unwrap();
    assert_eq!(resp_a.status(), StatusCode::CREATED, "AU-06");

    let req_b = build_request(
        RouteTarget::CreateInvite(TargetFamily::FamilyB),
        HeaderSpec::FamilyB,
        identity,
        family_a,
        family_b,
    )
    .unwrap();
    let resp_b = app.oneshot(req_b).await.unwrap();
    assert_problem_response(resp_b, StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE", "AU-06")
        .await
        .unwrap();
}

async fn run_auth_case(case: &AuthRuleCase) -> Result<(), String> {
    let (user_id, family_a, family_b) = (UserId::new_v4(), FamilyId::new_v4(), FamilyId::new_v4());
    let mut reader = FakeMembershipReader::default();
    if let Some((target, role)) = case.membership {
        let fam = resolve_family(target, family_a, family_b);
        reader.memberships.insert(
            (fam, user_id),
            MemberAccess {
                member_id: MemberId::new_v4(),
                role,
                permissions: case.permissions.to_vec(),
            },
        );
    }

    let identity = case.has_identity.then_some(UserIdentity {
        user_id,
        email: None,
    });
    let req = build_request(case.route, case.header, identity, family_a, family_b)?;
    let Ok(resp) = build_test_router(Arc::new(reader)).oneshot(req).await;

    match case.expected {
        ExpectedHttp::Status(status) if resp.status() == status => Ok(()),
        ExpectedHttp::Status(status) => Err(format!(
            "{}: expected {status}, got {}",
            case.id,
            resp.status()
        )),
        ExpectedHttp::Problem(status, code) => {
            assert_problem_response(resp, status, code, case.id).await
        }
    }
}

fn multi_family_test_app() -> (UserId, FamilyId, FamilyId, Router) {
    let (user_id, family_a, family_b) = (UserId::new_v4(), FamilyId::new_v4(), FamilyId::new_v4());
    let mut reader = FakeMembershipReader::default();
    reader.memberships.insert(
        (family_a, user_id),
        MemberAccess {
            member_id: MemberId::new_v4(),
            role: Role::Owner,
            permissions: Vec::new(),
        },
    );
    reader.memberships.insert(
        (family_b, user_id),
        MemberAccess {
            member_id: MemberId::new_v4(),
            role: Role::Child,
            permissions: Vec::new(),
        },
    );
    (
        user_id,
        family_a,
        family_b,
        build_test_router(Arc::new(reader)),
    )
}

fn build_test_router(reader: Arc<FakeMembershipReader>) -> Router {
    let mut router = Router::new();
    for policy in FAMILY_ROUTE_POLICIES {
        router = router.route(policy.path, route_handler(policy, Arc::clone(&reader)));
    }
    router
}

fn route_handler(policy: &RoutePolicy, reader: Arc<FakeMembershipReader>) -> MethodRouter {
    let success_status = if policy.method == "POST" {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    match policy.scope {
        RouteScope::IdentityOnly => identity_method_router(policy.method, success_status),
        RouteScope::FamilyScoped { required_action } => {
            let state = FamilyAuthState {
                membership_reader: reader,
                required_action,
            };
            family_method_router(policy.method, success_status).route_layer(
                middleware::from_fn_with_state(state, family_auth_middleware),
            )
        }
    }
}

fn identity_method_router(method: &str, status: StatusCode) -> MethodRouter {
    let handler = move |_: UserIdentity| async move { status };
    match method {
        "GET" => get(handler),
        "POST" => post(handler),
        _ => get(handler),
    }
}

fn family_method_router(method: &str, status: StatusCode) -> MethodRouter {
    let handler = move |_: AuthClaims| async move { status };
    match method {
        "GET" => get(handler),
        "PUT" => put(handler),
        "POST" => post(handler),
        "PATCH" => patch(handler),
        "DELETE" => delete(handler),
        _ => get(handler),
    }
}

fn build_request(
    route: RouteTarget,
    header_spec: HeaderSpec,
    identity: Option<UserIdentity>,
    family_a: FamilyId,
    family_b: FamilyId,
) -> Result<Request<Body>, String> {
    let (method, uri) = route_method_and_uri(route, family_a, family_b);
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(val) = header_value(header_spec, family_a, family_b) {
        builder = builder.header(FAMILY_ID_HEADER, val);
    }
    let mut req = builder.body(Body::empty()).map_err(|e| e.to_string())?;
    if let Some(id) = identity {
        req.extensions_mut().insert(id);
    }
    Ok(req)
}

fn route_method_and_uri(
    route: RouteTarget,
    family_a: FamilyId,
    family_b: FamilyId,
) -> (Method, String) {
    let Some((target, method, suffix)) = family_route_parts(route, uuid::Uuid::new_v4()) else {
        return match route {
            RouteTarget::CreateFamily => (Method::POST, "/families".to_owned()),
            _ => (Method::GET, "/profile/families".to_owned()),
        };
    };
    let family_id = resolve_family(target, family_a, family_b);
    (method, format!("/families/{family_id}{suffix}"))
}

fn family_route_parts(
    route: RouteTarget,
    member_id: uuid::Uuid,
) -> Option<(TargetFamily, Method, String)> {
    match route {
        RouteTarget::ProfileFamilies | RouteTarget::CreateFamily => None,
        RouteTarget::AcceptInvite(t) => Some((t, Method::POST, "/members".to_owned())),
        RouteTarget::GetFamily(t) => Some((t, Method::GET, String::new())),
        RouteTarget::GetMembers(t) => Some((t, Method::GET, "/members".to_owned())),
        RouteTarget::UpdateFamily(t) => Some((t, Method::PUT, String::new())),
        RouteTarget::CreateInvite(t) => Some((t, Method::POST, "/invites".to_owned())),
        RouteTarget::ChangeMemberRole(t) => {
            Some((t, Method::PATCH, format!("/members/{member_id}/role")))
        }
        RouteTarget::RemoveMember(t) => Some((t, Method::DELETE, format!("/members/{member_id}"))),
    }
}

fn resolve_family(target: TargetFamily, family_a: FamilyId, family_b: FamilyId) -> FamilyId {
    match target {
        TargetFamily::FamilyA => family_a,
        TargetFamily::FamilyB => family_b,
    }
}

fn header_value(spec: HeaderSpec, family_a: FamilyId, family_b: FamilyId) -> Option<String> {
    match spec {
        HeaderSpec::Absent => None,
        HeaderSpec::Empty => Some(String::new()),
        HeaderSpec::Malformed => Some("not-a-uuid".to_owned()),
        HeaderSpec::FamilyA => Some(family_a.to_string()),
        HeaderSpec::FamilyB => Some(family_b.to_string()),
    }
}

async fn assert_problem_response(
    resp: axum::response::Response,
    expected_status: StatusCode,
    expected_code: &'static str,
    id: &str,
) -> Result<(), String> {
    if resp.status() != expected_status {
        return Err(format!(
            "{id}: expected status {expected_status}, got {}",
            resp.status()
        ));
    }
    let ctype = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    if ctype != Some("application/problem+json") {
        return Err(format!("{id}: unexpected content-type {ctype:?}"));
    }
    let bytes = to_bytes(resp.into_body(), 16 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if body["code"] != expected_code {
        return Err(format!(
            "{id}: expected code {expected_code}, got {}",
            body["code"]
        ));
    }
    Ok(())
}

#[derive(Serialize)]
struct SignedClaims<'a> {
    sub: &'a str,
    iss: &'a str,
    exp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_use: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email_verified: Option<bool>,
}

fn make_signed_id_token(
    secret: &[u8],
    iss: &str,
    aud: &str,
    exp: i64,
) -> Result<String, String> {
    let sub = UserId::new_v4().to_string();
    let claims = SignedClaims {
        sub: &sub,
        iss,
        exp,
        token_use: Some("id"),
        aud: Some(aud),
        email: Some("a@b.com"),
        email_verified: Some(true),
    };
    let key = EncodingKey::from_secret(secret).map_err(|e| e.to_string())?;
    encode(&Header::new(Algorithm::HS256), &claims, &key).map_err(|e| e.to_string())
}

#[tokio::test]
async fn au_09_claims_require_sub_supported_token_use_and_verified_id_email() {
    let sub = UserId::new_v4().to_string();
    let bad_cases = [
        (HashMap::from([("sub".into(), "bad-uuid".into()), ("token_use".into(), "access".into())]), "INVALID_CLAIMS"),
        (HashMap::from([("sub".into(), sub.clone())]), "INVALID_CLAIMS"),
        (HashMap::from([("sub".into(), sub.clone()), ("token_use".into(), "refresh".into())]), "INVALID_CLAIMS"),
        (HashMap::from([("sub".into(), sub.clone()), ("token_use".into(), "id".into()), ("email".into(), "a@b.com".into())]), "UNVERIFIED_EMAIL"),
        (HashMap::from([("sub".into(), sub.clone()), ("token_use".into(), "id".into()), ("email".into(), "a@b.com".into()), ("email_verified".into(), "false".into())]), "UNVERIFIED_EMAIL"),
        (HashMap::from([("sub".into(), sub.clone()), ("token_use".into(), "id".into()), ("email_verified".into(), "true".into()), ("email".into(), "not-an-email".into())]), "INVALID_CLAIMS"),
    ];
    for (claims, expected_code) in bad_cases {
        let err = UserIdentity::from_claim_map(&claims).unwrap_err();
        assert_problem_response(err.into_response(), StatusCode::UNAUTHORIZED, expected_code, "AU-09")
            .await
            .unwrap();
    }
    let access = HashMap::from([
        ("sub".into(), sub.clone()),
        ("token_use".into(), "access".into()),
        ("email".into(), "ignored@b.com".into()),
    ]);
    assert!(UserIdentity::from_claim_map(&access).unwrap().email.is_none());
    let id = HashMap::from([
        ("sub".into(), sub),
        ("token_use".into(), "id".into()),
        ("email".into(), "alice@example.com".into()),
        ("email_verified".into(), "true".into()),
    ]);
    assert_eq!(
        UserIdentity::from_claim_map(&id).unwrap().email.as_ref().map(|e| e.as_str()),
        Some("alice@example.com")
    );
}

#[tokio::test]
async fn au_10_local_jwt_verifier_fails_closed_and_responses_carry_security_headers() {
    let secret = b"au10-local-verifier-test-secret-bytes";
    let verifier = LocalJwtVerifier::new(
        DecodingKey::from_secret(secret).unwrap(),
        Algorithm::HS256,
        "https://iss.local",
        "client-app",
    );
    let mut no_auth = HeaderMap::new();
    no_auth.insert("x-user-id", UserId::new_v4().to_string().parse().unwrap());
    let missing = verifier.verify_headers(&no_auth).unwrap_err().into_response();
    assert_eq!(missing.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");
    assert_eq!(missing.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
    assert_problem_response(missing, StatusCode::UNAUTHORIZED, "MISSING_CLAIMS", "AU-10")
        .await
        .unwrap();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let expired = make_signed_id_token(secret, "https://iss.local", "client-app", now - 120).unwrap();
    let bad_iss = make_signed_id_token(secret, "https://evil.local", "client-app", now + 300).unwrap();
    let bad_aud = make_signed_id_token(secret, "https://iss.local", "wrong-client", now + 300).unwrap();
    for raw in ["not-a-jwt", expired.as_str(), bad_iss.as_str(), bad_aud.as_str()] {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, format!("Bearer {raw}").parse().unwrap());
        let resp = verifier.verify_headers(&h).unwrap_err().into_response();
        assert_problem_response(resp, StatusCode::UNAUTHORIZED, "INVALID_CLAIMS", "AU-10")
            .await
            .unwrap();
    }
    let app = Router::new()
        .route("/ping", get(|| async { StatusCode::OK }))
        .layer(nosniff_layer())
        .layer(no_store_cache_layer());
    let ok = app.oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(ok.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");
    assert_eq!(ok.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
}

#[tokio::test]
async fn au_11_auth_claims_populates_member_id_and_permissions() {
    let (user_id, family_id, member_id) = (UserId::new_v4(), FamilyId::new_v4(), MemberId::new_v4());
    let mut reader = FakeMembershipReader::default();
    reader.memberships.insert(
        (family_id, user_id),
        MemberAccess {
            member_id,
            role: Role::Guest,
            permissions: vec![Permission::ViewAllAccounts, Permission::RecordOwnTransactions],
        },
    );
    let state = FamilyAuthState {
        membership_reader: Arc::new(reader),
        required_action: domain::value_objects::DomainAction::ViewFamilyWorkspace,
    };
    let app = Router::new()
        .route("/families/{id}", get(|claims: AuthClaims| async move {
            Json(serde_json::json!({
                "member_id": claims.member_id,
                "role": claims.role,
                "permissions": claims.permissions,
            }))
        }))
        .route_layer(middleware::from_fn_with_state(state, family_auth_middleware));
    let mut req = Request::builder()
        .uri(format!("/families/{family_id}"))
        .header(FAMILY_ID_HEADER, family_id.to_string())
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(UserIdentity { user_id, email: None });
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), 16 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["member_id"], member_id.to_string());
    assert_eq!(json["role"], "guest");
    assert_eq!(json["permissions"], serde_json::json!(["view_all_accounts", "record_own_transactions"]));
}

#[tokio::test]
async fn au_12_guest_without_accounts_or_ledger_flags_gets_403() {
    let (user_id, family_id) = (UserId::new_v4(), FamilyId::new_v4());
    let mut reader = FakeMembershipReader::default();
    reader.memberships.insert(
        (family_id, user_id),
        MemberAccess {
            member_id: MemberId::new_v4(),
            role: Role::Guest,
            permissions: vec![Permission::ViewBudget],
        },
    );
    let reader = Arc::new(reader);
    let mut router = Router::new();
    for policy in ACCOUNTS_ROUTE_POLICIES.iter().chain(LEDGER_ROUTE_POLICIES.iter()) {
        router = router.route(policy.path, route_handler(policy, Arc::clone(&reader)));
    }
    for (method, path) in [
        (Method::GET, "/accounts"),
        (Method::POST, "/accounts"),
        (Method::GET, "/categories"),
        (Method::GET, "/transactions"),
        (Method::POST, "/transactions"),
    ] {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header(FAMILY_ID_HEADER, family_id.to_string())
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(UserIdentity { user_id, email: None });
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_problem_response(resp, StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE", "AU-12")
            .await
            .unwrap();
    }
}
