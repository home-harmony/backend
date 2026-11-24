//! Invite and membership HTTP handlers for `family_service`.

use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};

use api::{
    auth::{AuthClaims, UserIdentity},
    errors::{ApiJson, domain_error_response},
    response::{created, no_content, ok},
};
use domain::{
    errors::DomainError,
    ports::{EventPublisher, FamilyRepository, TokenScope},
    value_objects::{MemberDisplayName, FamilyId, InviteTtl, MemberId, Relationship},
};

use super::{
    load_family_or_not_found, parse_and_verify_path_family_id, parse_family_and_member_paths,
    parse_path_family_id, publish_best_effort,
};
use crate::{
    app_state::AppState,
    dto::{
        AcceptInviteRequest, ChangeMemberRoleRequest, CreateInviteRequest, InviteResponse,
        ListMembersResponse, MemberResponse,
    },
};

const DEFAULT_INVITE_TTL_HOURS: i64 = 168;

/// `POST /families/{id}/invites` — generates a one-time invite token (`Role::Owner` only).
pub async fn post_family_invite<R, P>(
    State(state): State<AppState<R, P>>,
    claims: AuthClaims,
    Path(raw_id): Path<String>,
    ApiJson(req): ApiJson<CreateInviteRequest>,
) -> Response
where
    R: FamilyRepository + Send + Sync + 'static,
    P: EventPublisher + Send + Sync + 'static,
{
    if let Err(err) = parse_and_verify_path_family_id(&raw_id, claims.family_id) {
        return err.into_response();
    }
    match create_invite_workflow(&state, &claims, req).await {
        Ok(resp) => created(resp).into_response(),
        Err(err) => domain_error_response(err),
    }
}

/// `POST /families/{id}/members` — accepts an invite token and joins the family (identity-only).
pub async fn post_family_member<R, P>(
    State(state): State<AppState<R, P>>,
    identity: UserIdentity,
    Path(raw_id): Path<String>,
    ApiJson(req): ApiJson<AcceptInviteRequest>,
) -> Response
where
    R: FamilyRepository + Send + Sync + 'static,
    P: EventPublisher + Send + Sync + 'static,
{
    let family_id = match parse_path_family_id(&raw_id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    match accept_invite_workflow(&state, &identity, family_id, req).await {
        Ok(resp) => created(resp).into_response(),
        Err(err) => domain_error_response(err),
    }
}

/// `GET /families/{id}/members` — lists all active (`deleted_at IS NULL`) family members.
pub async fn get_family_members<R, P>(
    State(state): State<AppState<R, P>>,
    claims: AuthClaims,
    Path(raw_id): Path<String>,
) -> Response
where
    R: FamilyRepository + Send + Sync + 'static,
    P: Send + Sync + 'static,
{
    if let Err(err) = parse_and_verify_path_family_id(&raw_id, claims.family_id) {
        return err.into_response();
    }
    match list_members_workflow(&*state.repo, claims.family_id).await {
        Ok(resp) => ok(resp).into_response(),
        Err(err) => domain_error_response(err),
    }
}

/// `PATCH /families/{id}/members/{mid}/role` — updates member role and permissions (`Role::Owner` only).
pub async fn patch_member_role<R, P>(
    State(state): State<AppState<R, P>>,
    claims: AuthClaims,
    Path((raw_family_id, raw_member_id)): Path<(String, String)>,
    ApiJson(req): ApiJson<ChangeMemberRoleRequest>,
) -> Response
where
    R: FamilyRepository + Send + Sync + 'static,
    P: EventPublisher + Send + Sync + 'static,
{
    let member_id =
        match parse_family_and_member_paths(&raw_family_id, &raw_member_id, claims.family_id) {
            Ok(id) => id,
            Err(resp) => return *resp,
        };
    match change_member_role_workflow(&state, &claims, member_id, req).await {
        Ok(resp) => ok(resp).into_response(),
        Err(err) => domain_error_response(err),
    }
}

/// `DELETE /families/{id}/members/{mid}` — soft-deletes a family member (`Role::Owner` only).
pub async fn delete_member<R, P>(
    State(state): State<AppState<R, P>>,
    claims: AuthClaims,
    Path((raw_family_id, raw_member_id)): Path<(String, String)>,
) -> Response
where
    R: FamilyRepository + Send + Sync + 'static,
    P: EventPublisher + Send + Sync + 'static,
{
    let member_id =
        match parse_family_and_member_paths(&raw_family_id, &raw_member_id, claims.family_id) {
            Ok(id) => id,
            Err(resp) => return *resp,
        };
    match remove_member_workflow(&state, &claims, member_id).await {
        Ok(()) => no_content().into_response(),
        Err(err) => domain_error_response(err),
    }
}

async fn create_invite_workflow<R, P>(
    state: &AppState<R, P>,
    claims: &AuthClaims,
    req: CreateInviteRequest,
) -> Result<InviteResponse, DomainError>
where
    R: FamilyRepository,
    P: EventPublisher,
{
    let relationship = req.relationship.map(Relationship::try_new).transpose()?;
    let ttl = InviteTtl::from_hours(req.ttl_hours.unwrap_or(DEFAULT_INVITE_TTL_HOURS))?;
    let user_id = claims.user_id;
    let role = req.role;
    let permissions = req.permissions;

    let updated = state
        .repo
        .update(claims.family_id, TokenScope::None, move |family| {
            // Cloned per attempt because OCC retries may re-execute this closure.
            let (invite, events) = family.create_invite(
                user_id,
                role,
                permissions.clone(),
                relationship.clone(),
                ttl,
            )?;
            Ok((InviteResponse::from(&invite), events))
        })
        .await?;
    publish_best_effort(&*state.publisher, &updated.events).await;
    Ok(updated.value)
}

async fn accept_invite_workflow<R, P>(
    state: &AppState<R, P>,
    identity: &UserIdentity,
    family_id: FamilyId,
    req: AcceptInviteRequest,
) -> Result<MemberResponse, DomainError>
where
    R: FamilyRepository,
    P: EventPublisher,
{
    let display_name = MemberDisplayName::try_new(req.display_name)?;
    let user_id = identity.user_id;
    let token = req.token;
    let updated = state
        .repo
        .update(family_id, TokenScope::One(token), move |family| {
            // Cloned per attempt because OCC retries may re-execute this closure.
            let (member, events) = family.accept_invite(token, user_id, display_name.clone())?;
            Ok((MemberResponse::from(&member), events))
        })
        .await?;
    publish_best_effort(&*state.publisher, &updated.events).await;
    Ok(updated.value)
}

async fn list_members_workflow<R: FamilyRepository>(
    repo: &R,
    family_id: FamilyId,
) -> Result<ListMembersResponse, DomainError> {
    let family = load_family_or_not_found(repo, family_id).await?;
    if family.is_deleted() {
        return Err(DomainError::FamilyDeleted);
    }
    let items = family.active_members().map(MemberResponse::from).collect();
    Ok(ListMembersResponse { items })
}

async fn change_member_role_workflow<R, P>(
    state: &AppState<R, P>,
    claims: &AuthClaims,
    member_id: MemberId,
    req: ChangeMemberRoleRequest,
) -> Result<MemberResponse, DomainError>
where
    R: FamilyRepository,
    P: EventPublisher,
{
    let user_id = claims.user_id;
    let role = req.role;
    let permissions = req.permissions;
    let updated = state
        .repo
        .update(claims.family_id, TokenScope::None, move |family| {
            // Cloned per attempt because OCC retries may re-execute this closure.
            let events =
                family.change_member_role(user_id, member_id, role, permissions.clone())?;
            let member = family
                .active_members()
                .find(|m| m.id() == member_id)
                .ok_or(DomainError::MemberNotFound { member_id })?;
            Ok((MemberResponse::from(member), events))
        })
        .await?;
    publish_best_effort(&*state.publisher, &updated.events).await;
    Ok(updated.value)
}

async fn remove_member_workflow<R, P>(
    state: &AppState<R, P>,
    claims: &AuthClaims,
    member_id: MemberId,
) -> Result<(), DomainError>
where
    R: FamilyRepository,
    P: EventPublisher,
{
    let user_id = claims.user_id;
    let updated = state
        .repo
        .update(claims.family_id, TokenScope::None, move |family| {
            let events = family.remove_member(user_id, member_id)?;
            Ok(((), events))
        })
        .await?;
    publish_best_effort(&*state.publisher, &updated.events).await;
    Ok(())
}
