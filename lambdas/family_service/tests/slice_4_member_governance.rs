//! Layer 2b HTTP integration tests for Slice 4 (`PATCH /families/{id}/members/{mid}/role`, `DELETE /families/{id}/members/{mid}`).

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
use tower::ServiceExt;

use domain::{
    entities::{Family, FamilyParams},
    errors::DomainError,
    events::DomainEvent,
    ports::{FamilyRepository, MemberAccess, MembershipReader, UserFamilySummary},
    value_objects::{
        CurrencyCode, DisplayName, FamilyId, FamilyName, InviteTtl, MemberId, Permission, Role,
        UserId,
    },
};
use family_service::{AppState, build_router};
use infrastructure::events::SpyPublisher;

#[derive(Default)]
struct InMemoryFamilyRepo {
    families: Mutex<Vec<Family>>,
    fail_on_save: Option<String>,
}

struct RoleUsers {
    owner: UserId,
    owner_member_id: MemberId,
    member: UserId,
    member_id: MemberId,
    child: UserId,
    other: UserId,
    other_all_perms: UserId,
    other_all_perms_id: MemberId,
    removed_member: UserId,
    removed_member_id: MemberId,
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
    let repo = InMemoryFamilyRepo::failing_on_save(vec![family], "save must not run on no-op");
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

    let roster_req = get_req(&format!("/families/{fid}/members"), roles.owner, &fid)?;
    let (_, _, roster) = send_json(app.clone(), roster_req).await?;
    let items = roster["items"].as_array().context("items array")?;
    assert!(items.iter().all(|m| m["id"] != roles.member_id.to_string()));

    let revoked = get_req(&format!("/families/{fid}"), roles.member, &fid)?;
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

fn build_family_with_all_roles() -> Result<(Family, RoleUsers)> {
    let owner = UserId::new_v4();
    let (mut fam, _) = Family::create(
        FamilyName::try_new("Household")?,
        CurrencyCode::try_new("USD")?,
        owner,
        DisplayName::try_new("Owner")?,
    )?;
    let (member, member_id) = add_member(&mut fam, owner, Role::Member, vec![], "Member")?;
    let (child, _) = add_member(&mut fam, owner, Role::Child, vec![], "Child")?;
    let (other, _) = add_member(&mut fam, owner, Role::Other, vec![], "Other")?;
    let all = Permission::all().to_vec();
    let (other_all_perms, other_all_perms_id) =
        add_member(&mut fam, owner, Role::Other, all, "OtherAll")?;
    let (removed_member, removed_member_id) = add_removed_member(&mut fam, owner)?;
    let roles = RoleUsers {
        owner,
        owner_member_id: fam.members()[0].id(),
        member,
        member_id,
        child,
        other,
        other_all_perms,
        other_all_perms_id,
        removed_member,
        removed_member_id,
    };
    Ok((fam, roles))
}

fn add_removed_member(family: &mut Family, owner: UserId) -> Result<(UserId, MemberId)> {
    let (uid, mid) = add_member(family, owner, Role::Member, vec![], "Removed")?;
    family.remove_member(owner, mid)?;
    Ok((uid, mid))
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

fn add_member(
    family: &mut Family,
    owner: UserId,
    role: Role,
    permissions: Vec<Permission>,
    name: &str,
) -> Result<(UserId, MemberId)> {
    let uid = UserId::new_v4();
    let (inv, _) =
        family.create_invite(owner, role, permissions, None, InviteTtl::from_hours(24)?)?;
    let (joined, _) = family.accept_invite(inv.token(), uid, DisplayName::try_new(name)?)?;
    Ok((uid, joined.id()))
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

fn get_req(uri: &str, user_id: UserId, family_header: &str) -> Result<Request<Body>> {
    request_with_auth(Method::GET, uri, Some(user_id), Some(family_header), None)
}

fn patch_req(uri: &str, user_id: UserId, family_header: &str, body: &str) -> Result<Request<Body>> {
    request_with_auth(
        Method::PATCH,
        uri,
        Some(user_id),
        Some(family_header),
        Some(body),
    )
}

fn delete_req(uri: &str, user_id: UserId, family_header: &str) -> Result<Request<Body>> {
    request_with_auth(
        Method::DELETE,
        uri,
        Some(user_id),
        Some(family_header),
        None,
    )
}

fn request_with_auth(
    method: Method,
    uri: &str,
    user_id: Option<UserId>,
    family_header: Option<&str>,
    json_body: Option<&str>,
) -> Result<Request<Body>> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(fid) = family_header {
        builder = builder.header("x-family-id", fid);
    }
    let body = match json_body {
        Some(raw) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(raw.to_owned())
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
    let mut jwt = ApiGatewayRequestAuthorizerJwtDescription::default();
    jwt.claims = HashMap::from([("sub".to_owned(), sub.to_string())]);
    let mut authorizer = ApiGatewayRequestAuthorizer::default();
    authorizer.jwt = Some(jwt);
    let mut v2 = ApiGatewayV2httpRequestContext::default();
    v2.authorizer = Some(authorizer);
    RequestContext::ApiGatewayV2(v2)
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

async fn assert_problem(
    app: Router,
    req: Request<Body>,
    expected_status: StatusCode,
    expected_code: &str,
) -> Result<Value> {
    let (status, content_type, body) = send_json(app, req).await?;
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

async fn send_json(app: Router, req: Request<Body>) -> Result<(StatusCode, Option<String>, Value)> {
    let (status, content_type, bytes) = send_raw(app, req).await?;
    let body = serde_json::from_slice(&bytes)?;
    Ok((status, content_type, body))
}

async fn send_raw(
    app: Router,
    req: Request<Body>,
) -> Result<(StatusCode, Option<String>, Vec<u8>)> {
    let resp = app.oneshot(req).await?;
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await?.to_vec();
    Ok((status, content_type, bytes))
}
