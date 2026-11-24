//! Shared Layer 2b HTTP test harness and in-memory repository fake for `family_service`.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use chrono::{DateTime, Utc};
use lambda_http::{
    aws_lambda_events::apigw::{
        ApiGatewayRequestAuthorizer, ApiGatewayRequestAuthorizerJwtDescription,
        ApiGatewayV2httpRequestContext,
    },
    request::RequestContext,
};
use serde_json::Value;
use tokio::sync::Mutex;
use tower::ServiceExt;

use domain::{
    entities::{Family, FamilyMember, FamilyParams},
    errors::DomainError,
    events::DomainEvent,
    ports::{
        FamilyRepository, FamilySummary, MemberAccess, MemberSummary, MembershipReader, TokenScope,
        Updated, UserFamilySummary,
    },
    value_objects::{
        CurrencyCode, MemberDisplayName, FamilyId, FamilyName, InviteTokenId, InviteTtl, MemberId,
        Permission, Relationship, Role, UserId,
    },
};
use family_service::{AppState, build_router};
use infrastructure::events::SpyPublisher;

#[derive(Default)]
pub struct InMemoryFamilyRepo {
    families: Mutex<Vec<Family>>,
    fail_all: Option<String>,
    fail_on_write: Option<String>,
}

pub struct StubMissingFamilyRepo;

pub struct RoleUsers {
    pub owner: UserId,
    pub owner_member_id: MemberId,
    pub member: UserId,
    pub member_id: MemberId,
    pub child: UserId,
    pub other: UserId,
    pub other_all_perms: UserId,
    pub other_all_perms_id: MemberId,
    pub removed_member: UserId,
    pub removed_member_id: MemberId,
}

impl InMemoryFamilyRepo {
    pub fn with_families(families: Vec<Family>) -> Self {
        Self {
            families: Mutex::new(families),
            fail_all: None,
            fail_on_write: None,
        }
    }

    pub fn failing(message: impl Into<String>) -> Self {
        Self {
            families: Mutex::new(Vec::new()),
            fail_all: Some(message.into()),
            fail_on_write: None,
        }
    }

    pub fn failing_on_write(families: Vec<Family>, message: impl Into<String>) -> Self {
        Self {
            families: Mutex::new(families),
            fail_all: None,
            fail_on_write: Some(message.into()),
        }
    }

    pub async fn stored_family(&self, family_id: FamilyId) -> Option<Family> {
        self.families
            .lock()
            .await
            .iter()
            .find(|f| f.id() == family_id)
            .cloned()
    }

    fn check_fail_all(&self) -> Result<(), DomainError> {
        if let Some(message) = &self.fail_all {
            return Err(DomainError::Internal {
                message: message.clone(),
            });
        }
        Ok(())
    }

    fn check_fail_write(&self) -> Result<(), DomainError> {
        if let Some(message) = &self.fail_on_write {
            return Err(DomainError::Internal {
                message: message.clone(),
            });
        }
        Ok(())
    }
}

impl FamilyRepository for InMemoryFamilyRepo {
    async fn find_by_id(&self, family_id: FamilyId) -> Result<Option<Family>, DomainError> {
        self.check_fail_all()?;
        Ok(self
            .families
            .lock()
            .await
            .iter()
            .find(|f| f.id() == family_id)
            .map(|f| clone_with_scope(f, TokenScope::None)))
    }

    async fn list_for_user(&self, user_id: UserId) -> Result<Vec<UserFamilySummary>, DomainError> {
        self.check_fail_all()?;
        let guard = self.families.lock().await;
        Ok(guard
            .iter()
            .filter(|f| !f.is_deleted())
            .filter_map(|f| summarize_for_user(f, user_id))
            .collect())
    }

    async fn insert_new(&self, family: &Family) -> Result<(), DomainError> {
        self.check_fail_all()?;
        self.check_fail_write()?;
        let mut guard = self.families.lock().await;
        if guard.iter().any(|f| f.id() == family.id()) {
            return Err(DomainError::Internal {
                message: "Duplicate family ID in InMemoryFamilyRepo".to_owned(),
            });
        }
        guard.push(family.clone());
        Ok(())
    }

    async fn update<T, F>(
        &self,
        family_id: FamilyId,
        scope: TokenScope,
        op: F,
    ) -> Result<Updated<T>, DomainError>
    where
        T: Send,
        F: Fn(&mut Family) -> Result<(T, Vec<DomainEvent>), DomainError> + Send + Sync,
    {
        self.check_fail_all()?;
        let mut guard = self.families.lock().await;
        let idx = guard
            .iter()
            .position(|f| f.id() == family_id)
            .ok_or(DomainError::FamilyNotFound { family_id })?;
        let mut working = clone_with_scope(&guard[idx], scope);
        let (value, events) = op(&mut working)?;
        if !events.is_empty() || profile_changed(&guard[idx], &working) {
            self.check_fail_write()?;
            merge_back(&mut guard[idx], working);
        }
        Ok(Updated { value, events })
    }
}

impl MembershipReader for InMemoryFamilyRepo {
    async fn find_active_membership(
        &self,
        family_id: FamilyId,
        user_id: UserId,
    ) -> Result<Option<MemberAccess>, DomainError> {
        self.check_fail_all()?;
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

    async fn insert_new(&self, _family: &Family) -> Result<(), DomainError> {
        Ok(())
    }

    async fn update<T, F>(
        &self,
        family_id: FamilyId,
        _scope: TokenScope,
        _op: F,
    ) -> Result<Updated<T>, DomainError>
    where
        T: Send,
        F: Fn(&mut Family) -> Result<(T, Vec<DomainEvent>), DomainError> + Send + Sync,
    {
        Err(DomainError::FamilyNotFound { family_id })
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

pub fn test_app(repo: InMemoryFamilyRepo, spy: SpyPublisher) -> Router {
    build_router(AppState::new(Arc::new(repo), Arc::new(spy)))
}

pub fn build_family_with_all_roles() -> Result<(Family, RoleUsers)> {
    let owner = UserId::new_v4();
    let (mut fam, _) = Family::create(
        FamilyName::try_new("Household")?,
        CurrencyCode::try_new("USD")?,
        owner,
        MemberDisplayName::try_new("Owner")?,
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

pub fn add_member(
    family: &mut Family,
    owner: UserId,
    role: Role,
    permissions: Vec<Permission>,
    name: &str,
) -> Result<(UserId, MemberId)> {
    let uid = UserId::new_v4();
    let (inv, _) =
        family.create_invite(owner, role, permissions, None, InviteTtl::from_hours(24)?)?;
    let (joined, _) = family.accept_invite(inv.token(), uid, MemberDisplayName::try_new(name)?)?;
    Ok((uid, joined.id()))
}

pub fn add_removed_member(family: &mut Family, owner: UserId) -> Result<(UserId, MemberId)> {
    let (uid, mid) = add_member(family, owner, Role::Member, vec![], "Removed")?;
    family.remove_member(owner, mid)?;
    Ok((uid, mid))
}

pub fn seed_other_invite(
    family: &mut Family,
    owner: UserId,
    perm: Permission,
    relationship: &str,
) -> Result<InviteTokenId> {
    let (invite, _) = family.create_invite(
        owner,
        Role::Other,
        vec![perm],
        Some(Relationship::try_new(relationship)?),
        InviteTtl::from_hours(24)?,
    )?;
    Ok(invite.token())
}

pub fn mark_family_soft_deleted(family: &Family) -> Family {
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

pub fn parse_ts(val: &Value) -> Result<DateTime<Utc>> {
    let raw = val.as_str().context("timestamp must be a JSON string")?;
    Ok(DateTime::parse_from_rfc3339(raw)?.with_timezone(&Utc))
}

pub fn get_req(
    uri: &str,
    user_id: Option<UserId>,
    family_header: Option<&str>,
) -> Result<Request<Body>> {
    request_with_auth(Method::GET, uri, user_id, family_header, None)
}

pub fn put_req(
    uri: &str,
    user_id: UserId,
    family_header: &str,
    body: String,
) -> Result<Request<Body>> {
    request_with_auth(
        Method::PUT,
        uri,
        Some(user_id),
        Some(family_header),
        Some(&body),
    )
}

pub fn post_family_req(
    uri: &str,
    user_id: UserId,
    family_header: &str,
    body: &str,
) -> Result<Request<Body>> {
    request_with_auth(
        Method::POST,
        uri,
        Some(user_id),
        Some(family_header),
        Some(body),
    )
}

pub fn post_identity_req(uri: &str, user_id: Option<UserId>, body: &str) -> Result<Request<Body>> {
    request_with_auth(Method::POST, uri, user_id, None, Some(body))
}

pub fn patch_req(
    uri: &str,
    user_id: UserId,
    family_header: &str,
    body: &str,
) -> Result<Request<Body>> {
    request_with_auth(
        Method::PATCH,
        uri,
        Some(user_id),
        Some(family_header),
        Some(body),
    )
}

pub fn delete_req(uri: &str, user_id: UserId, family_header: &str) -> Result<Request<Body>> {
    request_with_auth(
        Method::DELETE,
        uri,
        Some(user_id),
        Some(family_header),
        None,
    )
}

pub fn request_with_auth(
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
        req.extensions_mut().insert(v2_jwt_context(uid, None, None));
    }
    Ok(req)
}

pub fn v2_jwt_context(
    sub: UserId,
    email: Option<&str>,
    stage: Option<&str>,
) -> RequestContext {
    request_with_sub(&sub.to_string(), email, stage)
}

pub fn request_with_sub(sub: &str, email: Option<&str>, stage: Option<&str>) -> RequestContext {
    let mut claims = HashMap::from([("sub".to_owned(), sub.to_owned())]);
    if let Some(addr) = email {
        claims.insert("email".to_owned(), addr.to_owned());
    }
    let mut jwt = ApiGatewayRequestAuthorizerJwtDescription::default();
    jwt.claims = claims;
    let mut authorizer = ApiGatewayRequestAuthorizer::default();
    authorizer.jwt = Some(jwt);
    let mut v2 = ApiGatewayV2httpRequestContext::default();
    v2.stage = stage.map(ToOwned::to_owned);
    v2.authorizer = Some(authorizer);
    RequestContext::ApiGatewayV2(v2)
}

pub async fn assert_problem(
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

pub async fn send_json(
    app: Router,
    req: Request<Body>,
) -> Result<(StatusCode, Option<String>, Value)> {
    let (status, content_type, bytes) = send_raw(app, req).await?;
    let body = serde_json::from_slice(&bytes)?;
    Ok((status, content_type, body))
}

pub async fn send_raw(
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

fn clone_with_scope(family: &Family, scope: TokenScope) -> Family {
    let invite_tokens = match scope {
        TokenScope::None => Vec::new(),
        TokenScope::One(target) => family
            .invite_tokens()
            .iter()
            .filter(|t| t.token() == target)
            .cloned()
            .collect(),
    };
    Family::reconstitute(FamilyParams {
        id: family.id(),
        name: family.name().clone(),
        home_currency: family.home_currency().clone(),
        members: family.members().to_vec(),
        invite_tokens,
        created_at: family.created_at(),
        deleted_at: family.deleted_at(),
    })
}

fn merge_back(stored: &mut Family, updated: Family) {
    let mut merged_tokens = stored.invite_tokens().to_vec();
    for tok in updated.invite_tokens() {
        if let Some(pos) = merged_tokens.iter().position(|t| t.token() == tok.token()) {
            merged_tokens[pos] = tok.clone();
        } else {
            merged_tokens.push(tok.clone());
        }
    }
    *stored = Family::reconstitute(FamilyParams {
        id: updated.id(),
        name: updated.name().clone(),
        home_currency: updated.home_currency().clone(),
        members: updated.members().to_vec(),
        invite_tokens: merged_tokens,
        created_at: updated.created_at(),
        deleted_at: updated.deleted_at(),
    });
}

fn profile_changed(before: &Family, after: &Family) -> bool {
    before.name() != after.name()
        || before.home_currency() != after.home_currency()
        || before.deleted_at() != after.deleted_at()
        || members_differ(before.members(), after.members())
}

fn members_differ(a: &[FamilyMember], b: &[FamilyMember]) -> bool {
    if a.len() != b.len() {
        return true;
    }
    a.iter().zip(b).any(|(x, y)| {
        x.id() != y.id()
            || x.display_name() != y.display_name()
            || x.role() != y.role()
            || x.permissions() != y.permissions()
            || x.deleted_at() != y.deleted_at()
    })
}

fn summarize_for_user(family: &Family, user_id: UserId) -> Option<UserFamilySummary> {
    family
        .active_members()
        .find(|m| m.user_id() == user_id)
        .map(|m| UserFamilySummary {
            family: FamilySummary {
                id: family.id(),
                name: family.name().clone(),
                home_currency: family.home_currency().clone(),
            },
            membership: MemberSummary {
                role: m.role(),
                display_name: m.display_name().clone(),
            },
        })
}
