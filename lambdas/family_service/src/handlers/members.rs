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
    ports::{EventPublisher, FamilyRepository},
    value_objects::{DisplayName, FamilyId, InviteTtl, MemberId, Relationship},
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
    P: EventPublisher + Send + Sync + 'static,
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

    let mut family = load_family_or_not_found(&*state.repo, claims.family_id).await?;
    let (invite, events) =
        family.create_invite(claims.user_id, req.role, req.permissions, relationship, ttl)?;
    state.repo.save(&family).await?;
    publish_best_effort(&*state.publisher, &events).await;
    Ok(InviteResponse::from(&invite))
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
    let display_name = DisplayName::try_new(req.display_name)?;
    let mut family = load_family_or_not_found(&*state.repo, family_id).await?;
    let (member, events) = family.accept_invite(req.token, identity.user_id, display_name)?;
    state.repo.save(&family).await?;
    publish_best_effort(&*state.publisher, &events).await;
    Ok(MemberResponse::from(&member))
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
    let mut family = load_family_or_not_found(&*state.repo, claims.family_id).await?;
    let events = family.change_member_role(claims.user_id, member_id, req.role, req.permissions)?;
    if !events.is_empty() {
        state.repo.save(&family).await?;
        publish_best_effort(&*state.publisher, &events).await;
    }
    let updated = family
        .active_members()
        .find(|m| m.id() == member_id)
        .ok_or(DomainError::MemberNotFound { member_id })?;
    Ok(MemberResponse::from(updated))
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
    let mut family = load_family_or_not_found(&*state.repo, claims.family_id).await?;
    let events = family.remove_member(claims.user_id, member_id)?;
    state.repo.save(&family).await?;
    publish_best_effort(&*state.publisher, &events).await;
    Ok(())
}
