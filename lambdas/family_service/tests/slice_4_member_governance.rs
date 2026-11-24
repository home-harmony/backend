//! Layer 2b HTTP integration tests for Slice 4 (`PATCH /families/{id}/members/{mid}/role`, `DELETE /families/{id}/members/{mid}`).

mod common;

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::http::{Method, StatusCode};
use serde_json::json;

use domain::{
    events::DomainEvent,
    ports::FamilyRepository,
    value_objects::{FamilyId, MemberId, Permission, Role, UserId},
};
use family_service::{AppState, build_router};
use infrastructure::events::SpyPublisher;

use common::{
    InMemoryFamilyRepo, assert_problem, build_family_with_all_roles, delete_req, get_req,
    mark_family_soft_deleted, patch_req, request_with_auth, send_json, send_raw, test_app,
};

#[tokio::test]
async fn patch_role_changes_member_to_other_deduplicating_permissions_and_emits_event() -> Result<()>
{
    let (family, roles) = build_family_with_all_roles()?;
    let family_id = family.id();
    let fid = family_id.to_string();
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(
        Arc::new(InMemoryFamilyRepo::with_families(vec![family])),
        Arc::clone(&spy),
    ));
    let uri = format!("/families/{fid}/members/{}/role", roles.member_id);
    let payload = r#"{"role":"other","permissions":["view_budget","view_reports","view_budget"]}"#;

    let (status, _, body) = send_json(app, patch_req(&uri, roles.owner, &fid, payload)?).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], roles.member_id.to_string());
    assert_eq!(body["family_id"], fid);
    assert_eq!(body["user_id"], roles.member.to_string());
    assert_eq!(body["display_name"], "Member");
    assert_eq!(body["role"], "other");
    assert_eq!(body["permissions"], json!(["view_budget", "view_reports"]));
    assert!(body["joined_at"].as_str().is_some());

    let expected_perms = [Permission::ViewBudget, Permission::ViewReports];
    assert_role_changed_event(
        &spy.events().await,
        family_id,
        roles.member_id,
        roles.owner_member_id,
        (Role::Member, Role::Other),
        &expected_perms,
    )
}

#[tokio::test]
async fn patch_role_clears_custom_permissions_when_transitioning_other_to_member() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let family_id = family.id();
    let fid = family_id.to_string();
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(
        Arc::new(InMemoryFamilyRepo::with_families(vec![family])),
        Arc::clone(&spy),
    ));
    let uri = format!("/families/{fid}/members/{}/role", roles.other_all_perms_id);

    let req = patch_req(
        &uri,
        roles.owner,
        &fid,
        r#"{"role":"member","permissions":[]}"#,
    )?;
    let (status, _, body) = send_json(app, req).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], roles.other_all_perms_id.to_string());
    assert_eq!(body["role"], "member");
    assert_eq!(body["permissions"], json!([]));

    assert_role_changed_event(
        &spy.events().await,
        family_id,
        roles.other_all_perms_id,
        roles.owner_member_id,
        (Role::Other, Role::Member),
        &[],
    )
}

#[tokio::test]
async fn patch_role_noop_returns_200_skips_save_and_emits_zero_events() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let repo = InMemoryFamilyRepo::failing_on_write(vec![family], "write must not run on no-op");
    let spy = Arc::new(SpyPublisher::failing("publish must not run on no-op"));
    let app = build_router(AppState::new(Arc::new(repo), Arc::clone(&spy)));
    let uri = format!("/families/{fid}/members/{}/role", roles.member_id);

    let req = patch_req(&uri, roles.owner, &fid, r#"{"role":"member"}"#)?;
    let (status, _, body) = send_json(app, req).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], roles.member_id.to_string());
    assert_eq!(body["role"], "member");
    assert!(spy.events().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn patch_role_blocks_owner_self_demotion_and_promoting_second_owner() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );

    let demote_uri = format!("/families/{fid}/members/{}/role", roles.owner_member_id);
    let demote = patch_req(&demote_uri, roles.owner, &fid, r#"{"role":"member"}"#)?;
    assert_problem(
        app.clone(),
        demote,
        StatusCode::CONFLICT,
        "CANNOT_REMOVE_OWNER",
    )
    .await?;

    let promote_uri = format!("/families/{fid}/members/{}/role", roles.member_id);
    let promote = patch_req(&promote_uri, roles.owner, &fid, r#"{"role":"owner"}"#)?;
    assert_problem(app, promote, StatusCode::CONFLICT, "DUPLICATE_OWNER").await?;
    Ok(())
}

#[tokio::test]
async fn patch_role_rejects_custom_permissions_for_member_and_child() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let uri = format!("/families/{fid}/members/{}/role", roles.member_id);
    let cases = [
        r#"{"role":"member","permissions":["view_budget"]}"#,
        r#"{"role":"child","permissions":["view_loans"]}"#,
    ];

    for raw in cases {
        let req = patch_req(&uri, roles.owner, &fid, raw)?;
        assert_problem(
            app.clone(),
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVARIANT_VIOLATION",
        )
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn patch_role_rejects_unknown_soft_deleted_and_non_uuid_member_id() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let cases = [
        (
            MemberId::new_v4().to_string(),
            StatusCode::NOT_FOUND,
            "MEMBER_NOT_FOUND",
        ),
        (
            roles.removed_member_id.to_string(),
            StatusCode::NOT_FOUND,
            "MEMBER_NOT_FOUND",
        ),
        (
            "not-a-uuid".to_owned(),
            StatusCode::BAD_REQUEST,
            "INVALID_MEMBER_ID",
        ),
    ];

    for (mid, status, code) in cases {
        let uri = format!("/families/{fid}/members/{mid}/role");
        let req = patch_req(&uri, roles.owner, &fid, r#"{"role":"child"}"#)?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn patch_role_rejects_non_owner_roles_with_403_and_zero_events() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(
        Arc::new(InMemoryFamilyRepo::with_families(vec![family])),
        Arc::clone(&spy),
    ));
    let uri = format!("/families/{fid}/members/{}/role", roles.member_id);

    for uid in [
        roles.member,
        roles.child,
        roles.other,
        roles.other_all_perms,
    ] {
        let req = patch_req(&uri, uid, &fid, r#"{"role":"child"}"#)?;
        assert_problem(app.clone(), req, StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE").await?;
    }
    assert!(spy.events().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn patch_role_rejects_path_mismatch_and_soft_deleted_family() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let deleted = mark_family_soft_deleted(&family);
    let fid = deleted.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![deleted]),
        SpyPublisher::new(),
    );

    let mismatch_uri = format!(
        "/families/{}/members/{}/role",
        FamilyId::new_v4(),
        roles.member_id
    );
    let mismatch = patch_req(&mismatch_uri, roles.owner, &fid, r#"{"role":"child"}"#)?;
    assert_problem(
        app.clone(),
        mismatch,
        StatusCode::FORBIDDEN,
        "FAMILY_ID_MISMATCH",
    )
    .await?;

    let deleted_uri = format!("/families/{fid}/members/{}/role", roles.member_id);
    let deleted_req = patch_req(&deleted_uri, roles.owner, &fid, r#"{"role":"child"}"#)?;
    assert_problem(app, deleted_req, StatusCode::GONE, "FAMILY_DELETED").await?;
    Ok(())
}

#[tokio::test]
async fn patch_role_rejects_bad_json_and_unknown_enums_with_400() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let uri = format!("/families/{fid}/members/{}/role", roles.member_id);
    let cases = [
        "{bad-json",
        r#"{"role":"admin"}"#,
        r#"{"role":"other","permissions":["ViewBudget"]}"#,
    ];

    for raw in cases {
        let req = patch_req(&uri, roles.owner, &fid, raw)?;
        assert_problem(app.clone(), req, StatusCode::BAD_REQUEST, "INVALID_JSON").await?;
    }
    Ok(())
}

#[tokio::test]
async fn patch_role_returns_200_when_eventbridge_publish_fails() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let family_id = family.id();
    let fid = family_id.to_string();
    let repo = Arc::new(InMemoryFamilyRepo::with_families(vec![family]));
    let app = build_router(AppState::new(
        Arc::clone(&repo),
        Arc::new(SpyPublisher::failing("EventBridge throttled")),
    ));
    let uri = format!("/families/{fid}/members/{}/role", roles.member_id);

    let req = patch_req(&uri, roles.owner, &fid, r#"{"role":"child"}"#)?;
    let (status, _, body) = send_json(app, req).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["role"], "child");
    let saved = repo.find_by_id(family_id).await?.context("family")?;
    let updated = saved
        .active_members()
        .find(|m| m.id() == roles.member_id)
        .context("updated member")?;
    assert_eq!(updated.role(), Role::Child);
    Ok(())
}

#[tokio::test]
async fn delete_member_soft_deletes_emits_event_hides_from_roster_and_revokes_access() -> Result<()>
{
    let (family, roles) = build_family_with_all_roles()?;
    let family_id = family.id();
    let fid = family_id.to_string();
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(
        Arc::new(InMemoryFamilyRepo::with_families(vec![family])),
        Arc::clone(&spy),
    ));

    let del_uri = format!("/families/{fid}/members/{}", roles.member_id);
    let (status, _, bytes) =
        send_raw(app.clone(), delete_req(&del_uri, roles.owner, &fid)?).await?;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty());
    let events = spy.events().await;
    assert_member_removed_event(&events, family_id, roles.member_id, roles.owner_member_id)?;

    let roster_req = get_req(&format!("/families/{fid}/members"), Some(roles.owner), Some(&fid))?;
    let (_, _, roster) = send_json(app.clone(), roster_req).await?;
    let items = roster["items"].as_array().context("items array")?;
    assert!(items.iter().all(|m| m["id"] != roles.member_id.to_string()));

    let revoked = get_req(&format!("/families/{fid}"), Some(roles.member), Some(&fid))?;
    assert_problem(app, revoked, StatusCode::FORBIDDEN, "USER_NOT_IN_FAMILY").await?;
    Ok(())
}

#[tokio::test]
async fn delete_member_blocks_owner_removal_with_409() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(
        Arc::new(InMemoryFamilyRepo::with_families(vec![family])),
        Arc::clone(&spy),
    ));

    let uri = format!("/families/{fid}/members/{}", roles.owner_member_id);
    let req = delete_req(&uri, roles.owner, &fid)?;
    assert_problem(app, req, StatusCode::CONFLICT, "CANNOT_REMOVE_OWNER").await?;
    assert!(spy.events().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn delete_member_rejects_unknown_already_deleted_and_non_uuid_member_id() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let cases = [
        (
            MemberId::new_v4().to_string(),
            StatusCode::NOT_FOUND,
            "MEMBER_NOT_FOUND",
        ),
        (
            roles.removed_member_id.to_string(),
            StatusCode::NOT_FOUND,
            "MEMBER_NOT_FOUND",
        ),
        (
            "not-a-uuid".to_owned(),
            StatusCode::BAD_REQUEST,
            "INVALID_MEMBER_ID",
        ),
    ];

    for (mid, status, code) in cases {
        let uri = format!("/families/{fid}/members/{mid}");
        let req = delete_req(&uri, roles.owner, &fid)?;
        assert_problem(app.clone(), req, status, code).await?;
    }
    Ok(())
}

#[tokio::test]
async fn delete_member_rejects_non_owner_roles_with_403_and_zero_events() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let spy = Arc::new(SpyPublisher::new());
    let app = build_router(AppState::new(
        Arc::new(InMemoryFamilyRepo::with_families(vec![family])),
        Arc::clone(&spy),
    ));
    let uri = format!("/families/{fid}/members/{}", roles.member_id);

    for uid in [
        roles.member,
        roles.child,
        roles.other,
        roles.other_all_perms,
    ] {
        let req = delete_req(&uri, uid, &fid)?;
        assert_problem(app.clone(), req, StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE").await?;
    }
    assert!(spy.events().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn delete_member_rejects_path_mismatch_and_soft_deleted_family() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let deleted = mark_family_soft_deleted(&family);
    let fid = deleted.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![deleted]),
        SpyPublisher::new(),
    );

    let other_uri = format!(
        "/families/{}/members/{}",
        FamilyId::new_v4(),
        roles.member_id
    );
    assert_problem(
        app.clone(),
        delete_req(&other_uri, roles.owner, &fid)?,
        StatusCode::FORBIDDEN,
        "FAMILY_ID_MISMATCH",
    )
    .await?;

    let uri = format!("/families/{fid}/members/{}", roles.member_id);
    assert_problem(
        app,
        delete_req(&uri, roles.owner, &fid)?,
        StatusCode::GONE,
        "FAMILY_DELETED",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn delete_member_returns_204_when_eventbridge_publish_fails() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let family_id = family.id();
    let fid = family_id.to_string();
    let repo = Arc::new(InMemoryFamilyRepo::with_families(vec![family]));
    let app = build_router(AppState::new(
        Arc::clone(&repo),
        Arc::new(SpyPublisher::failing("EventBridge throttled")),
    ));

    let uri = format!("/families/{fid}/members/{}", roles.member_id);
    let (status, _, bytes) = send_raw(app, delete_req(&uri, roles.owner, &fid)?).await?;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty());
    let saved = repo.find_by_id(family_id).await?.context("family")?;
    assert!(saved.active_members().all(|m| m.id() != roles.member_id));
    Ok(())
}

#[tokio::test]
async fn patch_and_delete_reject_missing_auth_and_family_header_errors() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let endpoints = governance_endpoints(&fid, roles.member_id);
    let (owner, ok_hdr) = (Some(roles.owner), Some(fid.as_str()));
    let cases = [
        (None, ok_hdr, StatusCode::UNAUTHORIZED, "MISSING_CLAIMS"),
        (owner, None, StatusCode::BAD_REQUEST, "MISSING_FAMILY_ID"),
        (
            owner,
            Some("bad"),
            StatusCode::BAD_REQUEST,
            "INVALID_FAMILY_ID",
        ),
    ];

    for (method, uri, body) in &endpoints {
        for (uid, hdr, status, code) in cases {
            let req = request_with_auth(method.clone(), uri, uid, hdr, *body)?;
            assert_problem(app.clone(), req, status, code).await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn patch_and_delete_reject_non_uuid_family_path_with_400() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );

    for (method, uri, body) in governance_endpoints("not-a-uuid", roles.member_id) {
        let req = request_with_auth(method, &uri, Some(roles.owner), Some(&fid), body)?;
        assert_problem(
            app.clone(),
            req,
            StatusCode::BAD_REQUEST,
            "INVALID_FAMILY_ID",
        )
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn patch_and_delete_reject_outsider_and_soft_deleted_member_with_403() -> Result<()> {
    let (family, roles) = build_family_with_all_roles()?;
    let fid = family.id().to_string();
    let app = test_app(
        InMemoryFamilyRepo::with_families(vec![family]),
        SpyPublisher::new(),
    );
    let endpoints = governance_endpoints(&fid, roles.member_id);

    for (method, uri, body) in &endpoints {
        for uid in [UserId::new_v4(), roles.removed_member] {
            let req = request_with_auth(method.clone(), uri, Some(uid), Some(&fid), *body)?;
            assert_problem(
                app.clone(),
                req,
                StatusCode::FORBIDDEN,
                "USER_NOT_IN_FAMILY",
            )
            .await?;
        }
    }
    Ok(())
}

fn governance_endpoints(
    family_segment: &str,
    member_id: MemberId,
) -> [(Method, String, Option<&'static str>); 2] {
    [
        (
            Method::PATCH,
            format!("/families/{family_segment}/members/{member_id}/role"),
            Some(r#"{"role":"child"}"#),
        ),
        (
            Method::DELETE,
            format!("/families/{family_segment}/members/{member_id}"),
            None,
        ),
    ]
}

fn assert_role_changed_event(
    events: &[DomainEvent],
    expected_family_id: FamilyId,
    expected_member_id: MemberId,
    expected_changed_by: MemberId,
    expected_roles: (Role, Role),
    expected_perms: &[Permission],
) -> Result<()> {
    anyhow::ensure!(events.len() == 1, "expected 1 event, got {}", events.len());
    let DomainEvent::MemberRoleChanged {
        family_id,
        member_id,
        old_role,
        new_role,
        permissions,
        changed_by,
        ..
    } = &events[0]
    else {
        anyhow::bail!("expected MemberRoleChanged event");
    };
    anyhow::ensure!(
        (*family_id, *member_id, *changed_by)
            == (expected_family_id, expected_member_id, expected_changed_by),
        "unexpected MemberRoleChanged IDs"
    );
    anyhow::ensure!(
        (*old_role, *new_role) == expected_roles && permissions.as_slice() == expected_perms,
        "unexpected MemberRoleChanged roles/permissions"
    );
    Ok(())
}

fn assert_member_removed_event(
    events: &[DomainEvent],
    expected_family_id: FamilyId,
    expected_member_id: MemberId,
    expected_removed_by: MemberId,
) -> Result<()> {
    anyhow::ensure!(events.len() == 1, "expected 1 event, got {}", events.len());
    let DomainEvent::MemberRemoved {
        family_id,
        member_id,
        removed_by,
        ..
    } = &events[0]
    else {
        anyhow::bail!("expected MemberRemoved event");
    };
    anyhow::ensure!(
        (*family_id, *member_id, *removed_by)
            == (expected_family_id, expected_member_id, expected_removed_by),
        "unexpected MemberRemoved IDs"
    );
    Ok(())
}
