//! Layer 2b HTTP integration tests for Slice 2 (`GET /families/{id}`, `PUT /families/{id}`).

mod common;

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use tower::{ServiceBuilder, ServiceExt};

use api::auth::strip_apigw_stage_prefix;
use domain::{
    entities::Family,
    ports::FamilyRepository,
    value_objects::{CurrencyCode, MemberDisplayName, FamilyId, FamilyName, InviteTtl, Role, UserId},
};
use family_service::{AppState, build_router};
use infrastructure::events::SpyPublisher;

use common::{
    InMemoryFamilyRepo, StubMissingFamilyRepo, assert_problem, build_family_with_all_roles,
    get_req, mark_family_soft_deleted, put_req, send_json, test_app, v2_jwt_context,
};

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
        let (status, _, body) = send_json(app.clone(), req).await?;
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
        .insert(v2_jwt_context(roles.owner, None, Some("dev")));

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
    let (status, _, body) = send_json(app, req).await?;
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
    let repo = InMemoryFamilyRepo::failing_on_write(vec![family], "secret dsql connection string");
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
        MemberDisplayName::try_new("Caller Owner")?,
    )?;
    let fam_b = create_family_with_member(UserId::new_v4(), caller, Role::Member)?;
    let (id_a, id_b) = (fam_a.id().to_string(), fam_b.id().to_string());
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![fam_a, fam_b]),
        SpyPublisher::new(),
    );
    let payload = json!({"name": "Renamed", "home_currency": "GBP"}).to_string();

    let ok_req = put_req(&format!("/families/{id_a}"), caller, &id_a, payload.clone())?;
    let (s_ok, _, _) = send_json(app.clone(), ok_req).await?;
    assert_eq!(s_ok, StatusCode::OK);

    let denied_req = put_req(&format!("/families/{id_b}"), caller, &id_b, payload)?;
    assert_problem(app, denied_req, StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE").await?;
    Ok(())
}

fn create_family_with_member(owner: UserId, member: UserId, role: Role) -> Result<Family> {
    let (mut fam, _) = Family::create(
        FamilyName::try_new("Family B")?,
        CurrencyCode::try_new("USD")?,
        owner,
        MemberDisplayName::try_new("Other Owner")?,
    )?;
    let (inv, _) = fam.create_invite(owner, role, vec![], None, InviteTtl::from_hours(24)?)?;
    fam.accept_invite(inv.token(), member, MemberDisplayName::try_new("Caller Member")?)?;
    Ok(fam)
}
