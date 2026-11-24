//! Layer 2b HTTP integration tests for Slice 2 (`GET /families/{id}`, `PUT /families/{id}`).

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use chrono::Utc;
use lambda_http::{
    aws_lambda_events::apigw::{
        ApiGatewayRequestAuthorizer, ApiGatewayRequestAuthorizerJwtDescription,
        ApiGatewayV2httpRequestContext,
    },
    request::RequestContext,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tower::{ServiceBuilder, ServiceExt};

use api::auth::strip_apigw_stage_prefix;
use domain::{
    entities::{Family, FamilyParams},
    errors::DomainError,
    ports::{FamilyRepository, MemberAccess, MembershipReader, UserFamilySummary},
    value_objects::{
        CurrencyCode, DisplayName, FamilyId, FamilyName, InviteTtl, Permission, Role, UserId,
    },
};
use family_service::{AppState, build_router};
use infrastructure::events::SpyPublisher;

#[derive(Default)]
struct InMemoryFamilyRepo {
    families: Mutex<Vec<Family>>,
    fail_on_save: Option<String>,
}

struct StubMissingFamilyRepo;

struct RoleUsers {
    owner: UserId,
    member: UserId,
    child: UserId,
    other: UserId,
    other_all_perms: UserId,
    removed_member: UserId,
}

impl InMemoryFamilyRepo {
    fn with_families(families: Vec<Family>) -> Self {
        Self {
            families: Mutex::new(families),
            fail_on_save: None,
        }
    }

    fn failing_on_save(families: Vec<Family>, message: impl Into<String>) -> Self {
        Self {
            families: Mutex::new(families),
            fail_on_save: Some(message.into()),
        }
    }
}

impl FamilyRepository for InMemoryFamilyRepo {
    async fn find_by_id(&self, family_id: FamilyId) -> Result<Option<Family>, DomainError> {
        Ok(self
            .families
            .lock()
            .await
            .iter()
            .find(|f| f.id() == family_id)
            .cloned())
    }

    async fn list_for_user(&self, _user_id: UserId) -> Result<Vec<UserFamilySummary>, DomainError> {
        Ok(Vec::new())
    }

    async fn save(&self, family: &Family) -> Result<(), DomainError> {
        if let Some(message) = &self.fail_on_save {
            return Err(DomainError::Internal {
                message: message.clone(),
            });
        }
        let mut guard = self.families.lock().await;
        if let Some(existing) = guard.iter_mut().find(|f| f.id() == family.id()) {
            *existing = family.clone();
        } else {
            guard.push(family.clone());
        }
        Ok(())
    }
}

impl MembershipReader for InMemoryFamilyRepo {
    async fn find_active_membership(
        &self,
        family_id: FamilyId,
        user_id: UserId,
    ) -> Result<Option<MemberAccess>, DomainError> {
        let guard = self.families.lock().await;
        Ok(guard
            .iter()
            .find(|f| f.id() == family_id)
            .and_then(|f| f.active_members().find(|m| m.user_id() == user_id))
            .map(|m| MemberAccess {
                role: m.role(),
                permissions: m.permissions().to_vec(),
            }))
    }
}

impl FamilyRepository for StubMissingFamilyRepo {
    async fn find_by_id(&self, _family_id: FamilyId) -> Result<Option<Family>, DomainError> {
        Ok(None)
    }

    async fn list_for_user(&self, _user_id: UserId) -> Result<Vec<UserFamilySummary>, DomainError> {
        Ok(Vec::new())
    }

    async fn save(&self, _family: &Family) -> Result<(), DomainError> {
        Ok(())
    }
}

impl MembershipReader for StubMissingFamilyRepo {
    async fn find_active_membership(
        &self,
        _family_id: FamilyId,
        _user_id: UserId,
    ) -> Result<Option<MemberAccess>, DomainError> {
        Ok(Some(MemberAccess {
            role: Role::Owner,
            permissions: Vec::new(),
        }))
    }
}

#[tokio::test]
async fn get_family_returns_200_for_all_four_active_roles() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let path = format!("/families/{fid}");

    for uid in [roles.owner, roles.member, roles.child, roles.other] {
        let req = get_req(&path, Some(uid), Some(&fid))?;
        let (status, _, body) = send(app.clone(), req).await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], fid);
        assert_eq!(body["name"], "Household");
        assert_eq!(body["home_currency"], "USD");
        assert!(body["created_at"].as_str().is_some());
    }
    Ok(())
}

#[tokio::test]
async fn get_family_strips_apigw_named_stage_prefix_and_returns_200() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = ServiceBuilder::new()
        .map_request(strip_apigw_stage_prefix)
        .service(test_app(
            InMemoryFamilyRepo::with_families(vec![family]),
            SpyPublisher::new(),
        ));

    let mut req = Request::builder()
        .method(Method::GET)
        .uri(format!("/dev/families/{fid}"))
        .header("x-family-id", &fid)
        .body(Body::empty())?;
    req.extensions_mut()
        .insert(v2_jwt_context_with_stage(roles.owner, Some("dev")));

    let resp = app.oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await?;
    let body: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(body["id"], fid);
    assert_eq!(body["name"], "Household");
    Ok(())
}

#[tokio::test]
async fn get_family_rejects_missing_empty_or_malformed_family_header() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let path = format!("/families/{}", family.id());
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let cases = [
        (None, StatusCode::BAD_REQUEST, "MISSING_FAMILY_ID"),
        (Some("   "), StatusCode::BAD_REQUEST, "MISSING_FAMILY_ID"),
        (
            Some("not-a-uuid"),
            StatusCode::BAD_REQUEST,
            "INVALID_FAMILY_ID",
        ),
    ];

    for (header_val, status, code) in cases {
        let req = get_req(&path, Some(roles.owner), header_val)?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn get_family_rejects_non_uuid_path_and_mismatched_family_id() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );

    let bad_path = get_req("/families/not-a-uuid", Some(roles.owner), Some(&fid))?;
    assert_problem(
        app.clone(),
        bad_path,
        StatusCode::BAD_REQUEST,
        "INVALID_FAMILY_ID",
    )
    .await?;

    let other_path = format!("/families/{}", FamilyId::new_v4());
    let mismatch = get_req(&other_path, Some(roles.owner), Some(&fid))?;
    assert_problem(app, mismatch, StatusCode::FORBIDDEN, "FAMILY_ID_MISMATCH").await?;
    Ok(())
}

#[tokio::test]
async fn get_family_rejects_unauthenticated_non_member_and_soft_deleted_member() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let path = format!("/families/{fid}");
    let cases = [
        (None, StatusCode::UNAUTHORIZED, "MISSING_CLAIMS"),
        (
            Some(UserId::new_v4()),
            StatusCode::FORBIDDEN,
            "USER_NOT_IN_FAMILY",
        ),
        (
            Some(roles.removed_member),
            StatusCode::FORBIDDEN,
            "USER_NOT_IN_FAMILY",
        ),
    ];

    for (uid, status, code) in cases {
        let req = get_req(&path, uid, Some(&fid))?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn get_family_returns_410_when_soft_deleted_and_404_when_missing() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let deleted = mark_family_soft_deleted(&family);
    let fid = deleted.id().to_string();
    let path = format!("/families/{fid}");

    let deleted_app = test_app(
        InMemoryFamilyRepo::with_families(vec![deleted]),
        SpyPublisher::new(),
    );
    let req_410 = get_req(&path, Some(roles.owner), Some(&fid))?;
    assert_problem(deleted_app, req_410, StatusCode::GONE, "FAMILY_DELETED").await?;

    let missing_app = build_router(AppState::new(
        Arc::new(StubMissingFamilyRepo),
        Arc::new(SpyPublisher::new()),
    ));
    let req_404 = get_req(&path, Some(roles.owner), Some(&fid))?;
    assert_problem(
        missing_app,
        req_404,
        StatusCode::NOT_FOUND,
        "FAMILY_NOT_FOUND",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn put_family_updates_profile_for_owner_and_emits_zero_events() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let family_id = family.id();
    let fid = family_id.to_string();
    let repo = Arc::new(InMemoryFamilyRepo::with_families(vec![family]));
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(Arc::clone(&repo), Arc::clone(&spy)));
    let body_str = json!({"name": "  Updated Home  ", "home_currency": "EUR"}).to_string();

    let req = put_req(&format!("/families/{fid}"), roles.owner, &fid, body_str)?;
    let (status, _, body) = send(app, req).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], fid);
    assert_eq!(body["name"], "Updated Home");
    assert_eq!(body["home_currency"], "EUR");
    assert!(body["created_at"].as_str().is_some());

    let saved = repo.find_by_id(family_id).await?.context("saved family")?;
    assert_eq!(saved.name().as_str(), "Updated Home");
    assert_eq!(saved.home_currency().as_str(), "EUR");
    assert!(spy.events().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn put_family_rejects_member_child_and_other_with_all_permissions() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let family_id = family.id();
    let fid = family_id.to_string();
    let repo = Arc::new(InMemoryFamilyRepo::with_families(vec![family]));
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(Arc::clone(&repo), Arc::clone(&spy)));
    let path = format!("/families/{fid}");
    let payload = json!({"name": "Hijacked", "home_currency": "EUR"}).to_string();

    for uid in [roles.member, roles.child, roles.other_all_perms] {
        let req = put_req(&path, uid, &fid, payload.clone())?;
        assert_problem(app.clone(), req, StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE").await?;
    }

    let unchanged = repo.find_by_id(family_id).await?.context("family")?;
    assert_eq!(unchanged.name().as_str(), "Household");
    assert!(spy.events().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn put_family_rejects_soft_deleted_family_and_path_errors() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let deleted = mark_family_soft_deleted(&family);
    let fid = deleted.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![deleted]),
        SpyPublisher::new(),
    );
    let payload = r#"{"name":"Renamed","home_currency":"EUR"}"#;
    let cases = [
        (
            format!("/families/{fid}"),
            StatusCode::GONE,
            "FAMILY_DELETED",
        ),
        (
            format!("/families/{}", FamilyId::new_v4()),
            StatusCode::FORBIDDEN,
            "FAMILY_ID_MISMATCH",
        ),
        (
            "/families/not-a-uuid".to_owned(),
            StatusCode::BAD_REQUEST,
            "INVALID_FAMILY_ID",
        ),
    ];

    for (path, status, code) in cases {
        let req = put_req(&path, roles.owner, &fid, payload.to_owned())?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn put_family_returns_422_for_invalid_fields_and_400_for_bad_json() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let path = format!("/families/{fid}");
    let cases = [
        (
            r#"{"name":"  ","home_currency":"USD"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_FAMILY_NAME",
        ),
        (
            r#"{"name":"Home","home_currency":"usd"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_CURRENCY_CODE",
        ),
        ("{bad-json", StatusCode::BAD_REQUEST, "INVALID_JSON"),
        (
            r#"{"name":"Home"}"#,
            StatusCode::BAD_REQUEST,
            "INVALID_JSON",
        ),
    ];

    for (raw, status, code) in cases {
        let req = put_req(&path, roles.owner, &fid, raw.to_owned())?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn put_family_returns_500_sanitized_on_repository_internal_error() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let repo = InMemoryFamilyRepo::failing_on_save(vec![family], "secret dsql connection string");
    let app = test_app(repo, SpyPublisher::new());
    let payload = json!({"name": "New Name", "home_currency": "EUR"}).to_string();

    let req = put_req(&format!("/families/{fid}"), roles.owner, &fid, payload)?;
    let body = assert_problem(
        app,
        req,
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
    )
    .await?;
    assert_eq!(body["detail"], "An internal server error occurred");
    assert!(
        !body["detail"]
            .as_str()
            .context("detail must be a string")?
            .contains("secret")
    );
    Ok(())
}

#[tokio::test]
async fn put_family_enforces_multi_family_role_isolation() -> Result<()> {
    let caller = UserId::new_v4();
    let (fam_a, _) = Family::create(
        FamilyName::try_new("Family A")?,
        CurrencyCode::try_new("USD")?,
        caller,
        DisplayName::try_new("Caller Owner")?,
    )?;
    let fam_b = create_family_with_member(UserId::new_v4(), caller, Role::Member)?;
    let (id_a, id_b) = (fam_a.id().to_string(), fam_b.id().to_string());
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![fam_a, fam_b]),
        SpyPublisher::new(),
    );
    let payload = json!({"name": "Renamed", "home_currency": "GBP"}).to_string();

    let ok_req = put_req(&format!("/families/{id_a}"), caller, &id_a, payload.clone())?;
    let (s_ok, _, _) = send(app.clone(), ok_req).await?;
    assert_eq!(s_ok, StatusCode::OK);

    let denied_req = put_req(&format!("/families/{id_b}"), caller, &id_b, payload)?;
    assert_problem(app, denied_req, StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE").await?;
    Ok(())
}

fn build_family_with_all_roles() -> Result<(Family, RoleUsers)> {
    let owner = UserId::new_v4();
    let (mut fam, _) = Family::create(
        FamilyName::try_new("Household")?,
        CurrencyCode::try_new("USD")?,
        owner,
        DisplayName::try_new("Owner")?,
    )?;
    let roles = RoleUsers {
        owner,
        member: add_member(&mut fam, owner, Role::Member, vec![], "Member")?,
        child: add_member(&mut fam, owner, Role::Child, vec![], "Child")?,
        other: add_member(&mut fam, owner, Role::Other, vec![], "Other")?,
        other_all_perms: add_member(
            &mut fam,
            owner,
            Role::Other,
            Permission::all().to_vec(),
            "OtherAll",
        )?,
        removed_member: add_removed_member(&mut fam, owner)?,
    };
    Ok((fam, roles))
}

fn add_member(
    family: &mut Family,
    owner: UserId,
    role: Role,
    permissions: Vec<Permission>,
    name: &str,
) -> Result<UserId> {
    let uid = UserId::new_v4();
    let (inv, _) =
        family.create_invite(owner, role, permissions, None, InviteTtl::from_hours(24)?)?;
    family.accept_invite(inv.token(), uid, DisplayName::try_new(name)?)?;
    Ok(uid)
}

fn add_removed_member(family: &mut Family, owner: UserId) -> Result<UserId> {
    let uid = UserId::new_v4();
    let (inv, _) = family.create_invite(
        owner,
        Role::Member,
        vec![],
        None,
        InviteTtl::from_hours(24)?,
    )?;
    let (joined, _) = family.accept_invite(inv.token(), uid, DisplayName::try_new("Removed")?)?;
    family.remove_member(owner, joined.id())?;
    Ok(uid)
}

fn create_family_with_member(owner: UserId, member: UserId, role: Role) -> Result<Family> {
    let (mut fam, _) = Family::create(
        FamilyName::try_new("Family B")?,
        CurrencyCode::try_new("USD")?,
        owner,
        DisplayName::try_new("Other Owner")?,
    )?;
    let (inv, _) = fam.create_invite(owner, role, vec![], None, InviteTtl::from_hours(24)?)?;
    fam.accept_invite(inv.token(), member, DisplayName::try_new("Caller Member")?)?;
    Ok(fam)
}

fn mark_family_soft_deleted(family: &Family) -> Family {
    Family::reconstitute(FamilyParams {
        id: family.id(),
        name: family.name().clone(),
        home_currency: family.home_currency().clone(),
        members: family.members().to_vec(),
        invite_tokens: family.invite_tokens().to_vec(),
        created_at: family.created_at(),
        deleted_at: Some(Utc::now()),
    })
}

fn test_app(repo: InMemoryFamilyRepo, spy: SpyPublisher) -> Router {
    build_router(AppState::new(Arc::new(repo), Arc::new(spy)))
}

fn get_req(
    uri: &str,
    user_id: Option<UserId>,
    family_header: Option<&str>,
) -> Result<Request<Body>> {
    request_with_auth(Method::GET, uri, user_id, family_header, None)
}

fn put_req(uri: &str, user_id: UserId, family_header: &str, body: String) -> Result<Request<Body>> {
    request_with_auth(
        Method::PUT,
        uri,
        Some(user_id),
        Some(family_header),
        Some(body),
    )
}

fn request_with_auth(
    method: Method,
    uri: &str,
    user_id: Option<UserId>,
    family_header: Option<&str>,
    json_body: Option<String>,
) -> Result<Request<Body>> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(fid) = family_header {
        builder = builder.header("x-family-id", fid);
    }
    let body = match json_body {
        Some(raw) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(raw)
        }
        None => Body::empty(),
    };
    let mut req = builder.body(body)?;
    if let Some(uid) = user_id {
        req.extensions_mut().insert(v2_jwt_context(uid));
    }
    Ok(req)
}

fn v2_jwt_context(sub: UserId) -> RequestContext {
    v2_jwt_context_with_stage(sub, None)
}

fn v2_jwt_context_with_stage(sub: UserId, stage: Option<&str>) -> RequestContext {
    let mut jwt = ApiGatewayRequestAuthorizerJwtDescription::default();
    jwt.claims = HashMap::from([("sub".to_owned(), sub.to_string())]);
    let mut authorizer = ApiGatewayRequestAuthorizer::default();
    authorizer.jwt = Some(jwt);
    let mut v2 = ApiGatewayV2httpRequestContext::default();
    v2.stage = stage.map(ToOwned::to_owned);
    v2.authorizer = Some(authorizer);
    RequestContext::ApiGatewayV2(v2)
}

async fn assert_problem(
    app: Router,
    req: Request<Body>,
    expected_status: StatusCode,
    expected_code: &str,
) -> Result<Value> {
    let (status, content_type, body) = send(app, req).await?;
    anyhow::ensure!(
        status == expected_status,
        "expected {expected_status}, got {status} (body: {body})"
    );
    anyhow::ensure!(
        content_type.as_deref() == Some("application/problem+json"),
        "expected application/problem+json, got {content_type:?}"
    );
    anyhow::ensure!(
        body["code"] == expected_code,
        "expected error code {expected_code}, got {}",
        body["code"]
    );
    Ok(body)
}

async fn send(app: Router, req: Request<Body>) -> Result<(StatusCode, Option<String>, Value)> {
    let resp = app.oneshot(req).await?;
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await?;
    let body = serde_json::from_slice(&bytes)?;
    Ok((status, content_type, body))
}
