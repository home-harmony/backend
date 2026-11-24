//! Layer 2b HTTP integration tests for Slice 1 (`POST /families`, `GET /profile/families`).

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use lambda_http::{
    aws_lambda_events::apigw::{
        ApiGatewayProxyRequestContext, ApiGatewayRequestAuthorizer,
        ApiGatewayRequestAuthorizerJwtDescription, ApiGatewayV2httpRequestContext,
    },
    request::RequestContext,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tower::ServiceExt;

use domain::{
    entities::Family,
    errors::DomainError,
    events::DomainEvent,
    ports::{FamilyRepository, MemberAccess, MembershipReader, UserFamilySummary},
    value_objects::{FamilyId, Role, UserId},
};
use family_service::{AppState, build_router};
use infrastructure::events::SpyPublisher;

#[derive(Default)]
struct InMemoryFamilyRepo {
    families: Mutex<Vec<Family>>,
    fail_with: Option<String>,
}

impl InMemoryFamilyRepo {
    fn failing(message: impl Into<String>) -> Self {
        Self {
            families: Mutex::new(Vec::new()),
            fail_with: Some(message.into()),
        }
    }

    fn check_failure(&self) -> Result<(), DomainError> {
        if let Some(message) = &self.fail_with {
            return Err(DomainError::Internal {
                message: message.clone(),
            });
        }
        Ok(())
    }
}

impl FamilyRepository for InMemoryFamilyRepo {
    async fn find_by_id(&self, family_id: FamilyId) -> Result<Option<Family>, DomainError> {
        self.check_failure()?;
        Ok(self
            .families
            .lock()
            .await
            .iter()
            .find(|f| f.id() == family_id)
            .cloned())
    }

    async fn list_for_user(&self, user_id: UserId) -> Result<Vec<UserFamilySummary>, DomainError> {
        self.check_failure()?;
        let guard = self.families.lock().await;
        Ok(guard
            .iter()
            .filter(|f| !f.is_deleted())
            .filter_map(|f| summarize_for_user(f, user_id))
            .collect())
    }

    async fn save(&self, family: &Family) -> Result<(), DomainError> {
        self.check_failure()?;
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
        self.check_failure()?;
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

#[tokio::test]
async fn post_families_creates_family_persists_owner_and_publishes_events() -> Result<()> {
    let repo = Arc::new(InMemoryFamilyRepo::default());
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(Arc::clone(&repo), Arc::clone(&spy)));

    let owner_uid = UserId::new_v4();
    let req = post_json_with_context(
        "/families",
        &json!({"name": "  Smith Family  ", "home_currency": "USD", "owner_display_name": "Alice"}),
        v2_jwt_context(&owner_uid.to_string(), Some("alice@example.com")),
    )?;
    let (status, _, body) = send(app, req).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["name"], "Smith Family");
    assert_eq!(body["home_currency"], "USD");
    assert!(body["owner_member_id"].as_str().is_some());
    assert!(body["created_at"].as_str().is_some());

    let family_id: FamilyId = body["id"].as_str().unwrap().parse()?;
    let saved = repo.find_by_id(family_id).await?.expect("saved family");
    assert_eq!(saved.members().len(), 1);
    assert_eq!(saved.members()[0].role(), Role::Owner);

    let events = spy.events().await;
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], DomainEvent::FamilyCreated { .. }));
    assert!(matches!(events[1], DomainEvent::MemberJoined { .. }));
    Ok(())
}

#[tokio::test]
async fn post_families_accepts_access_token_context_without_email() -> Result<()> {
    let app = test_app(InMemoryFamilyRepo::default(), SpyPublisher::new());
    let uid = UserId::new_v4().to_string();
    let req = post_json_with_context(
        "/families",
        &json!({"name": "Second Home", "home_currency": "EUR", "owner_display_name": "Alice"}),
        v2_jwt_context(&uid, None),
    )?;
    let (status, _, body) = send(app, req).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["home_currency"], "EUR");
    Ok(())
}

#[tokio::test]
async fn post_families_rejects_missing_or_malformed_auth_with_401() -> Result<()> {
    let app = test_app(InMemoryFamilyRepo::default(), SpyPublisher::new());
    let valid_body = json!({"name": "Home", "home_currency": "USD", "owner_display_name": "Al"});

    let no_ctx = raw_post("/families", valid_body.to_string(), None)?;
    let (s1, ct1, b1) = send(app.clone(), no_ctx).await?;
    assert_eq!(
        (s1, ct1.as_deref()),
        (StatusCode::UNAUTHORIZED, Some("application/problem+json"))
    );
    assert_eq!(b1["code"], "MISSING_CLAIMS");

    let v1_ctx = post_json_with_context(
        "/families",
        &valid_body,
        RequestContext::ApiGatewayV1(ApiGatewayProxyRequestContext::default()),
    )?;
    let (s2, _, b2) = send(app.clone(), v1_ctx).await?;
    assert_eq!(
        (s2, b2["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("MISSING_CLAIMS"))
    );

    let bad_sub =
        post_json_with_context("/families", &valid_body, v2_jwt_context("not-a-uuid", None))?;
    let (s3, _, b3) = send(app, bad_sub).await?;
    assert_eq!(
        (s3, b3["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("INVALID_CLAIMS"))
    );
    Ok(())
}

#[tokio::test]
async fn post_families_returns_422_for_invalid_domain_fields() -> Result<()> {
    let app = test_app(InMemoryFamilyRepo::default(), SpyPublisher::new());
    let uid = UserId::new_v4().to_string();
    let cases = [
        (
            json!({"name": "   ", "home_currency": "USD", "owner_display_name": "Alice"}),
            "INVALID_FAMILY_NAME",
        ),
        (
            json!({"name": "Home", "home_currency": "usd", "owner_display_name": "Alice"}),
            "INVALID_CURRENCY_CODE",
        ),
        (
            json!({"name": "Home", "home_currency": "USD", "owner_display_name": ""}),
            "INVALID_DISPLAY_NAME",
        ),
    ];

    for (payload, expected_code) in cases {
        let req = post_json_with_context("/families", &payload, v2_jwt_context(&uid, None))?;
        let (status, ct, body) = send(app.clone(), req).await?;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(ct.as_deref(), Some("application/problem+json"));
        assert_eq!(body["code"], expected_code);
    }
    Ok(())
}

#[tokio::test]
async fn post_families_returns_400_invalid_json_for_malformed_or_incomplete_body() -> Result<()> {
    let app = test_app(InMemoryFamilyRepo::default(), SpyPublisher::new());
    let uid = UserId::new_v4().to_string();

    let malformed = raw_post(
        "/families",
        "{broken-json".to_owned(),
        Some(v2_jwt_context(&uid, None)),
    )?;
    let (s1, ct1, b1) = send(app.clone(), malformed).await?;
    assert_eq!(
        (s1, ct1.as_deref()),
        (StatusCode::BAD_REQUEST, Some("application/problem+json"))
    );
    assert_eq!(b1["code"], "INVALID_JSON");

    let missing_field = post_json_with_context(
        "/families",
        &json!({"name": "Home"}),
        v2_jwt_context(&uid, None),
    )?;
    let (s2, ct2, b2) = send(app, missing_field).await?;
    assert_eq!(
        (s2, ct2.as_deref()),
        (StatusCode::BAD_REQUEST, Some("application/problem+json"))
    );
    assert_eq!(b2["code"], "INVALID_JSON");
    Ok(())
}

#[tokio::test]
async fn post_families_returns_201_when_eventbridge_publish_fails() -> Result<()> {
    let repo = Arc::new(InMemoryFamilyRepo::default());
    let failing_spy = Arc::new(SpyPublisher::failing("EventBridge throttled"));
    let app = build_router(AppState::new(Arc::clone(&repo), failing_spy));

    let uid = UserId::new_v4().to_string();
    let req = post_json_with_context(
        "/families",
        &json!({"name": "Resilient Family", "home_currency": "USD", "owner_display_name": "Alice"}),
        v2_jwt_context(&uid, None),
    )?;
    let (status, _, body) = send(app, req).await?;
    assert_eq!(status, StatusCode::CREATED);
    let family_id: FamilyId = body["id"].as_str().unwrap().parse()?;
    assert!(repo.find_by_id(family_id).await?.is_some());
    Ok(())
}

#[tokio::test]
async fn post_families_returns_500_sanitized_on_repository_internal_error() -> Result<()> {
    let app = test_app(
        InMemoryFamilyRepo::failing("aurora dsql internal credentials leaked"),
        SpyPublisher::new(),
    );
    let uid = UserId::new_v4().to_string();
    let req = post_json_with_context(
        "/families",
        &json!({"name": "Failing Save", "home_currency": "USD", "owner_display_name": "Alice"}),
        v2_jwt_context(&uid, None),
    )?;
    let (status, ct, body) = send(app, req).await?;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(ct.as_deref(), Some("application/problem+json"));
    assert_eq!(body["code"], "INTERNAL_ERROR");
    assert_eq!(body["detail"], "An internal server error occurred");
    assert!(!body["detail"].as_str().unwrap().contains("credentials"));
    Ok(())
}

#[tokio::test]
async fn get_profile_families_returns_401_when_unauthenticated() -> Result<()> {
    let app = test_app(InMemoryFamilyRepo::default(), SpyPublisher::new());
    let req = Request::builder()
        .method("GET")
        .uri("/profile/families")
        .body(Body::empty())?;
    let (status, ct, body) = send(app, req).await?;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(ct.as_deref(), Some("application/problem+json"));
    assert_eq!(body["code"], "MISSING_CLAIMS");
    Ok(())
}

#[tokio::test]
async fn get_profile_families_returns_empty_list_for_new_user() -> Result<()> {
    let app = test_app(InMemoryFamilyRepo::default(), SpyPublisher::new());
    let uid = UserId::new_v4().to_string();
    let (status, _, body) = send(
        app,
        get_with_context("/profile/families", v2_jwt_context(&uid, None))?,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"], json!([]));
    Ok(())
}

#[tokio::test]
async fn get_profile_families_returns_only_callers_active_families() -> Result<()> {
    let app = test_app(InMemoryFamilyRepo::default(), SpyPublisher::new());
    let user_a = UserId::new_v4().to_string();
    let user_b = UserId::new_v4().to_string();

    seed_family_via_http(&app, &user_a, "Family One", "USD", "Alice A").await?;
    seed_family_via_http(&app, &user_a, "Family Two", "EUR", "Alice E").await?;
    seed_family_via_http(&app, &user_b, "Other User Home", "GBP", "Bob").await?;

    let (status, _, body) = send(
        app,
        get_with_context("/profile/families", v2_jwt_context(&user_a, None))?,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"]
        .as_array()
        .expect("items must be a JSON array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["name"], "Family One");
    assert_eq!(items[0]["home_currency"], "USD");
    assert_eq!(items[0]["role"], "owner");
    assert_eq!(items[0]["display_name"], "Alice A");
    assert_eq!(items[1]["name"], "Family Two");
    assert_eq!(items[1]["home_currency"], "EUR");
    Ok(())
}

fn summarize_for_user(family: &Family, user_id: UserId) -> Option<UserFamilySummary> {
    family
        .active_members()
        .find(|m| m.user_id() == user_id)
        .map(|m| UserFamilySummary {
            family_id: family.id(),
            name: family.name().clone(),
            home_currency: family.home_currency().clone(),
            role: m.role(),
            display_name: m.display_name().clone(),
        })
}

fn test_app(repo: InMemoryFamilyRepo, spy: SpyPublisher) -> Router {
    build_router(AppState::new(Arc::new(repo), Arc::new(spy)))
}

async fn seed_family_via_http(
    app: &Router,
    uid: &str,
    name: &str,
    currency: &str,
    display_name: &str,
) -> Result<()> {
    let req = post_json_with_context(
        "/families",
        &json!({"name": name, "home_currency": currency, "owner_display_name": display_name}),
        v2_jwt_context(uid, None),
    )?;
    let (status, _, _) = send(app.clone(), req).await?;
    anyhow::ensure!(
        status == StatusCode::CREATED,
        "expected 201 Created, got {status}"
    );
    Ok(())
}

fn v2_jwt_context(sub: &str, email: Option<&str>) -> RequestContext {
    let mut claims = HashMap::from([("sub".to_owned(), sub.to_owned())]);
    if let Some(addr) = email {
        claims.insert("email".to_owned(), addr.to_owned());
    }
    let mut jwt = ApiGatewayRequestAuthorizerJwtDescription::default();
    jwt.claims = claims;
    let mut authorizer = ApiGatewayRequestAuthorizer::default();
    authorizer.jwt = Some(jwt);
    let mut v2 = ApiGatewayV2httpRequestContext::default();
    v2.authorizer = Some(authorizer);
    RequestContext::ApiGatewayV2(v2)
}

fn post_json_with_context(
    uri: &str,
    payload: &Value,
    ctx: RequestContext,
) -> Result<Request<Body>> {
    raw_post(uri, payload.to_string(), Some(ctx))
}

fn raw_post(uri: &str, body: String, ctx: Option<RequestContext>) -> Result<Request<Body>> {
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))?;
    if let Some(context) = ctx {
        req.extensions_mut().insert(context);
    }
    Ok(req)
}

fn get_with_context(uri: &str, ctx: RequestContext) -> Result<Request<Body>> {
    let mut req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())?;
    req.extensions_mut().insert(ctx);
    Ok(req)
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
