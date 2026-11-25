//! Layer 2b HTTP integration tests for Slice 3 (`POST /families/{id}/invites`, `POST/GET /families/{id}/members`).

mod common;

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::http::{Method, StatusCode};
use chrono::{Duration, Utc};
use serde_json::json;

use domain::{
    entities::Family,
    events::DomainEvent,
    value_objects::{FamilyId, InviteTokenId, InviteTtl, Permission, Role, UserId},
};
use family_service::{AppState, build_router};
use infrastructure::events::SpyPublisher;

use common::{
    InMemoryFamilyRepo, assert_problem, build_family_with_all_roles, get_req,
    mark_family_soft_deleted, parse_ts, post_family_req, post_identity_req, request_with_auth,
    seed_other_invite, send_json, test_app,
};

#[tokio::test]
async fn post_invite_creates_member_invite_with_default_ttl_and_emits_event() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let repo = Arc::new(InMemoryFamilyRepo::with_families(vec![family]));
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(Arc::clone(&repo), Arc::clone(&spy)));

    let req = post_family_req(
        &format!("/families/{fid}/invites"),
        roles.owner,
        &fid,
        r#"{"role":"member"}"#,
    )?;
    let (status, _, body) = send_json(app, req).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["family_id"], fid);
    assert_eq!(body["role"], "member");
    assert_eq!(body["permissions"], json!([]));
    assert!(body["relationship"].is_null());
    assert_eq!(body["created_by"], roles.owner_member_id.to_string());
    assert_eq!(
        parse_ts(&body["expires_at"])? - parse_ts(&body["created_at"])?,
        Duration::hours(168)
    );

    let events = spy.events().await;
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], DomainEvent::MemberInvited { .. }));
    Ok(())
}

#[tokio::test]
async fn post_invite_deduplicates_other_permissions_trims_relationship_and_uses_custom_ttl()
-> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let payload = json!({
        "role": "other",
        "permissions": ["view_budget", "view_reports", "view_budget"],
        "relationship": "  Grandma  ",
        "ttl_hours": 48
    })
    .to_string();

    let req = post_family_req(
        &format!("/families/{fid}/invites"),
        roles.owner,
        &fid,
        &payload,
    )?;
    let (status, _, body) = send_json(app, req).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["role"], "other");
    assert_eq!(body["permissions"], json!(["view_budget", "view_reports"]));
    assert_eq!(body["relationship"], "Grandma");
    assert_eq!(
        parse_ts(&body["expires_at"])? - parse_ts(&body["created_at"])?,
        Duration::hours(48)
    );
    Ok(())
}

#[tokio::test]
async fn post_invite_rejects_non_owner_roles_with_403_and_zero_events() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(
        Arc::new(InMemoryFamilyRepo::with_families(vec![family])),
        Arc::clone(&spy),
    ));
    let path = format!("/families/{fid}/invites");

    for uid in [
        roles.member,
        roles.child,
        roles.other,
        roles.other_all_perms,
    ] {
        let req = post_family_req(&path, uid, &fid, r#"{"role":"member"}"#)?;
        assert_problem(app.clone(), req, StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE").await?;
    }
    assert!(spy.events().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn post_invite_rejects_owner_role_and_non_other_custom_permissions() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let path = format!("/families/{fid}/invites");
    let cases = [
        (
            r#"{"role":"owner"}"#,
            StatusCode::CONFLICT,
            "DUPLICATE_OWNER",
        ),
        (
            r#"{"role":"member","permissions":["view_budget"]}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVARIANT_VIOLATION",
        ),
        (
            r#"{"role":"child","permissions":["view_loans"]}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVARIANT_VIOLATION",
        ),
    ];

    for (raw, status, code) in cases {
        let req = post_family_req(&path, roles.owner, &fid, raw)?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn post_invite_rejects_invalid_ttl_hours_and_blank_relationship() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let path = format!("/families/{fid}/invites");
    let cases = [
        (r#"{"role":"member","ttl_hours":0}"#, "INVALID_INVITE_TTL"),
        (r#"{"role":"member","ttl_hours":-1}"#, "INVALID_INVITE_TTL"),
        (r#"{"role":"member","ttl_hours":721}"#, "INVALID_INVITE_TTL"),
        (
            r#"{"role":"member","relationship":"   "}"#,
            "INVALID_RELATIONSHIP",
        ),
    ];

    for (raw, code) in cases {
        let req = post_family_req(&path, roles.owner, &fid, raw)?;
        assert_problem(app.clone(), req, StatusCode::UNPROCESSABLE_ENTITY, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn post_invite_rejects_soft_deleted_family_with_410() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let deleted = mark_family_soft_deleted(&family);
    let fid = deleted.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![deleted]),
        SpyPublisher::new(),
    );

    let req = post_family_req(
        &format!("/families/{fid}/invites"),
        roles.owner,
        &fid,
        r#"{"role":"member"}"#,
    )?;
    assert_problem(app, req, StatusCode::GONE, "FAMILY_DELETED").await?;
    Ok(())
}

#[tokio::test]
async fn post_invite_returns_201_when_eventbridge_publish_fails() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let family_id = family.id();
    let fid = family_id.to_string();
    let repo = Arc::new(InMemoryFamilyRepo::with_families(vec![family]));
    let app = build_router(AppState::new(
        Arc::clone(&repo),
        Arc::new(SpyPublisher::failing("EventBridge throttled")),
    ));

    let req = post_family_req(
        &format!("/families/{fid}/invites"),
        roles.owner,
        &fid,
        r#"{"role":"child"}"#,
    )?;
    let (status, _, _) = send_json(app, req).await?;
    assert_eq!(status, StatusCode::CREATED);
    let saved = repo.stored_family(family_id).await.context("family")?;
    assert!(!saved.invite_tokens().is_empty());
    Ok(())
}

#[tokio::test]
async fn post_invite_rejects_unknown_enums_and_malformed_json_with_400() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let path = format!("/families/{fid}/invites");
    let cases = [
        r#"{"role":"admin"}"#,
        r#"{"role":"other","permissions":["ViewBudget"]}"#,
        "{broken-json",
    ];

    for raw in cases {
        let req = post_family_req(&path, roles.owner, &fid, raw)?;
        assert_problem(app.clone(), req, StatusCode::BAD_REQUEST, "INVALID_JSON").await?;
    }
    Ok(())
}

#[tokio::test]
async fn post_invite_accepts_max_ttl_720_and_rejects_overlong_relationship() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let path = format!("/families/{fid}/invites");

    let req_ok = post_family_req(
        &path,
        roles.owner,
        &fid,
        r#"{"role":"member","ttl_hours":720}"#,
    )?;
    let (status, _, body) = send_json(app.clone(), req_ok).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        parse_ts(&body["expires_at"])? - parse_ts(&body["created_at"])?,
        Duration::hours(720)
    );

    let too_long = json!({"role": "member", "relationship": "a".repeat(51)}).to_string();
    let req_err = post_family_req(&path, roles.owner, &fid, &too_long)?;
    assert_problem(
        app,
        req_err,
        StatusCode::UNPROCESSABLE_ENTITY,
        "INVALID_RELATIONSHIP",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn post_invite_rejects_missing_and_invalid_family_header() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let path = format!("/families/{}/invites", family.id());
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let body = Some(r#"{"role":"member"}"#);
    let cases = [
        (None, "MISSING_FAMILY_ID"),
        (Some("bad"), "INVALID_FAMILY_ID"),
    ];

    for (header_val, code) in cases {
        let req = request_with_auth(Method::POST, &path, Some(roles.owner), header_val, body)?;
        assert_problem(app.clone(), req, StatusCode::BAD_REQUEST, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn post_invite_rejects_non_member_and_path_mismatch() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let path = format!("/families/{fid}/invites");
    let other_path = format!("/families/{}/invites", FamilyId::new_v4());
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let body = r#"{"role":"member"}"#;

    let non_member = post_family_req(&path, UserId::new_v4(), &fid, body)?;
    assert_problem(
        app.clone(),
        non_member,
        StatusCode::FORBIDDEN,
        "USER_NOT_IN_FAMILY",
    )
    .await?;

    let mismatch = post_family_req(&other_path, roles.owner, &fid, body)?;
    assert_problem(app, mismatch, StatusCode::FORBIDDEN, "FAMILY_ID_MISMATCH").await?;
    Ok(())
}

#[tokio::test]
async fn post_member_accepts_invite_without_family_header_and_emits_member_joined() -> Result<()> {
    let (mut family, roles) = build_family_with_all_roles()?;
    let token = seed_other_invite(&mut family, roles.owner, Permission::ViewBudget, "Cousin")?;
    let fid = family.id().to_string();
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(
        Arc::new(InMemoryFamilyRepo::with_families(vec![family])),
        Arc::clone(&spy),
    ));

    let newcomer = UserId::new_v4();
    let payload = json!({"token": token, "display_name": "  Charlie  "}).to_string();
    let req = post_identity_req(
        &format!("/families/{fid}/members"),
        Some(newcomer),
        &payload,
    )?;
    let (status, _, body) = send_json(app, req).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["family_id"], fid);
    assert_eq!(body["user_id"], newcomer.to_string());
    assert_eq!(body["display_name"], "Charlie");
    assert_eq!(body["role"], "other");
    assert_eq!(body["permissions"], json!(["view_budget"]));
    assert_eq!(body["relationship"], "Cousin");
    assert!(body["joined_at"].as_str().is_some());

    let events = spy.events().await;
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], DomainEvent::MemberJoined { .. }));
    Ok(())
}

#[tokio::test]
async fn post_member_marks_token_used_and_returns_201_when_publish_fails() -> Result<()> {
    let (mut family, roles) = build_family_with_all_roles()?;
    let token = seed_other_invite(&mut family, roles.owner, Permission::ViewBudget, "Sibling")?;
    let family_id = family.id();
    let fid = family_id.to_string();
    let repo = Arc::new(InMemoryFamilyRepo::with_families(vec![family]));
    let app = build_router(AppState::new(
        Arc::clone(&repo),
        Arc::new(SpyPublisher::failing("EventBridge throttled")),
    ));

    let newcomer = UserId::new_v4();
    let payload = json!({"token": token, "display_name": "Sam"}).to_string();
    let req = post_identity_req(
        &format!("/families/{fid}/members"),
        Some(newcomer),
        &payload,
    )?;
    let (status, _, _) = send_json(app, req).await?;
    assert_eq!(status, StatusCode::CREATED);

    let saved = repo.stored_family(family_id).await.context("family")?;
    let used_invite = saved
        .invite_tokens()
        .iter()
        .find(|t| t.token() == token)
        .context("accepted invite token")?;
    assert!(used_invite.is_used());
    assert!(saved.active_members().any(|m| m.user_id() == newcomer));
    Ok(())
}

#[tokio::test]
async fn post_member_reactivates_soft_deleted_member_preserving_member_id() -> Result<()> {
    let (mut family, roles) = build_family_with_all_roles()?;
    let token = seed_other_invite(&mut family, roles.owner, Permission::ViewLoans, "Aunt")?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );

    let payload = json!({"token": token, "display_name": "Back Again"}).to_string();
    let req = post_identity_req(
        &format!("/families/{fid}/members"),
        Some(roles.removed_member),
        &payload,
    )?;
    let (status, _, body) = send_json(app, req).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["id"], roles.removed_member_id.to_string());
    assert_eq!(body["display_name"], "Back Again");
    assert_eq!(body["role"], "other");
    assert_eq!(body["permissions"], json!(["view_loans"]));
    assert_eq!(body["relationship"], "Aunt");
    Ok(())
}

#[tokio::test]
async fn post_member_rejects_unauthenticated_and_non_uuid_path() -> Result<()> {
    let (family, _) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let body = json!({"token": InviteTokenId::new_v4(), "display_name": "Alex"}).to_string();

    let unauth = post_identity_req(&format!("/families/{fid}/members"), None, &body)?;
    assert_problem(
        app.clone(),
        unauth,
        StatusCode::UNAUTHORIZED,
        "MISSING_CLAIMS",
    )
    .await?;

    let bad_path = post_identity_req(
        "/families/not-a-uuid/members",
        Some(UserId::new_v4()),
        &body,
    )?;
    assert_problem(app, bad_path, StatusCode::BAD_REQUEST, "INVALID_FAMILY_ID").await?;
    Ok(())
}

#[tokio::test]
async fn post_member_rejects_unknown_and_soft_deleted_family() -> Result<()> {
    let (family, _) = build_family_with_all_roles()?;
    let deleted = mark_family_soft_deleted(&family);
    let deleted_fid = deleted.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![deleted]),
        SpyPublisher::new(),
    );
    let body = json!({"token": InviteTokenId::new_v4(), "display_name": "Alex"}).to_string();
    let uid = Some(UserId::new_v4());

    let missing_uri = format!("/families/{}/members", FamilyId::new_v4());
    let req_404 = post_identity_req(&missing_uri, uid, &body)?;
    assert_problem(
        app.clone(),
        req_404,
        StatusCode::NOT_FOUND,
        "FAMILY_NOT_FOUND",
    )
    .await?;

    let req_410 = post_identity_req(&format!("/families/{deleted_fid}/members"), uid, &body)?;
    assert_problem(app, req_410, StatusCode::GONE, "FAMILY_DELETED").await?;
    Ok(())
}

#[tokio::test]
async fn post_member_rejects_unknown_used_and_expired_tokens() -> Result<()> {
    let (mut family, roles) = build_family_with_all_roles()?;
    let (used_tok, expired_tok, _) = seed_token_scenarios(&mut family, roles.owner)?;
    let path = format!("/families/{}/members", family.id());
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let uid = Some(UserId::new_v4());
    let cases = [
        (
            InviteTokenId::new_v4(),
            StatusCode::NOT_FOUND,
            "INVITE_TOKEN_NOT_FOUND",
        ),
        (used_tok, StatusCode::CONFLICT, "TOKEN_ALREADY_USED"),
        (expired_tok, StatusCode::GONE, "TOKEN_EXPIRED"),
    ];

    for (token, status, code) in cases {
        let raw = json!({"token": token, "display_name": "Alex"}).to_string();
        let req = post_identity_req(&path, uid, &raw)?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn post_member_rejects_already_active_member_without_consuming_token() -> Result<()> {
    let (mut family, roles) = build_family_with_all_roles()?;
    let (_, _, fresh_tok) = seed_token_scenarios(&mut family, roles.owner)?;
    let family_id = family.id();
    let path = format!("/families/{family_id}/members");
    let repo = Arc::new(InMemoryFamilyRepo::with_families(vec![family]));
    let app = build_router(AppState::new(
        Arc::clone(&repo),
        Arc::new(SpyPublisher::new()),
    ));

    let raw = json!({"token": fresh_tok, "display_name": "Alex"}).to_string();
    let req = post_identity_req(&path, Some(roles.member), &raw)?;
    assert_problem(app, req, StatusCode::CONFLICT, "MEMBER_ALREADY_EXISTS").await?;

    let stored = repo.stored_family(family_id).await.context("family")?;
    let still_unused = stored
        .invite_tokens()
        .iter()
        .find(|t| t.token() == fresh_tok)
        .context("fresh token")?;
    assert!(!still_unused.is_used());
    Ok(())
}

#[tokio::test]
async fn post_member_rejects_valid_token_from_another_family() -> Result<()> {
    let (mut family_a, roles_a) = build_family_with_all_roles()?;
    let (family_b, _) = build_family_with_all_roles()?;
    let token_a = seed_other_invite(&mut family_a, roles_a.owner, Permission::ViewBudget, "Aunt")?;
    let (id_a, id_b) = (family_a.id(), family_b.id());
    let repo = Arc::new(InMemoryFamilyRepo::with_families(vec![family_a, family_b]));
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(Arc::clone(&repo), Arc::clone(&spy)));

    let newcomer = UserId::new_v4();
    let payload = json!({"token": token_a, "display_name": "Alex"}).to_string();
    let bad_req = post_identity_req(
        &format!("/families/{id_b}/members"),
        Some(newcomer),
        &payload,
    )?;
    assert_problem(
        app.clone(),
        bad_req,
        StatusCode::NOT_FOUND,
        "INVITE_TOKEN_NOT_FOUND",
    )
    .await?;
    assert!(spy.events().await.is_empty());

    let ok_req = post_identity_req(
        &format!("/families/{id_a}/members"),
        Some(newcomer),
        &payload,
    )?;
    let (status, _, _) = send_json(app, ok_req).await?;
    assert_eq!(status, StatusCode::CREATED);
    Ok(())
}

#[tokio::test]
async fn post_member_rejects_blank_display_name_and_bad_json() -> Result<()> {
    let (family, _) = build_family_with_all_roles()?;
    let path = format!("/families/{}/members", family.id());
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let uid = Some(UserId::new_v4());
    let cases = [
        (
            json!({"token": InviteTokenId::new_v4(), "display_name": "   "}).to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_DISPLAY_NAME",
        ),
        (
            "{bad-json".to_owned(),
            StatusCode::BAD_REQUEST,
            "INVALID_JSON",
        ),
        (
            r#"{"display_name":"Alex"}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "INVALID_JSON",
        ),
    ];

    for (raw, status, code) in cases {
        let req = post_identity_req(&path, uid, &raw)?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn get_members_lists_only_active_members_for_all_four_active_roles() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let path = format!("/families/{fid}/members");

    for uid in [roles.owner, roles.member, roles.child, roles.other] {
        let req = get_req(&path, Some(uid), Some(&fid))?;
        let (status, _, body) = send_json(app.clone(), req).await?;
        assert_eq!(status, StatusCode::OK);
        let items = body["items"].as_array().context("items array")?;
        assert_eq!(items.len(), 5);
        let removed_id = roles.removed_member_id.to_string();
        assert!(items.iter().all(|item| item["id"] != removed_id));
        assert!(items.iter().all(|item| item["family_id"] == fid));
        assert!(
            items
                .iter()
                .all(|item| item["joined_at"].as_str().is_some())
        );
    }
    Ok(())
}

#[tokio::test]
async fn get_members_rejects_missing_malformed_header_and_path_mismatch() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let path = format!("/families/{fid}/members");
    let other_path = format!("/families/{}/members", FamilyId::new_v4());
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let cases = [
        (&path, None, StatusCode::BAD_REQUEST, "MISSING_FAMILY_ID"),
        (
            &path,
            Some("bad"),
            StatusCode::BAD_REQUEST,
            "INVALID_FAMILY_ID",
        ),
        (
            &other_path,
            Some(fid.as_str()),
            StatusCode::FORBIDDEN,
            "FAMILY_ID_MISMATCH",
        ),
    ];

    for (uri, header_val, status, code) in cases {
        let req = get_req(uri, Some(roles.owner), header_val)?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn get_members_rejects_non_member_removed_member_and_deleted_family() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let deleted = mark_family_soft_deleted(&family);
    let fid = family.id().to_string();
    let path = format!("/families/{fid}/members");
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );

    for uid in [UserId::new_v4(), roles.removed_member] {
        let req = get_req(&path, Some(uid), Some(&fid))?;
        assert_problem(
            app.clone(),
            req,
            StatusCode::FORBIDDEN,
            "USER_NOT_IN_FAMILY",
        )
        .await?;
    }
    let deleted_app = test_app(
        InMemoryFamilyRepo::with_families(vec![deleted]),
        SpyPublisher::new(),
    );
    let req_410 = get_req(&path, Some(roles.owner), Some(&fid))?;
    assert_problem(deleted_app, req_410, StatusCode::GONE, "FAMILY_DELETED").await?;
    Ok(())
}

fn seed_token_scenarios(
    family: &mut Family,
    owner: UserId,
) -> Result<(InviteTokenId, InviteTokenId, InviteTokenId)> {
    let used_tok = family.invite_tokens()[0].token();
    let past = Utc::now() - Duration::hours(48);
    let (expired, _) = family.create_invite_at(
        owner,
        Role::Member,
        vec![],
        None,
        InviteTtl::from_hours(1)?,
        past,
    )?;
    let (fresh, _) = family.create_invite(
        owner,
        Role::Member,
        vec![],
        None,
        InviteTtl::from_hours(24)?,
    )?;
    Ok((used_tok, expired.token(), fresh.token()))
}
