//! Family workspace HTTP handlers (`POST /families`, `GET /profile/families`, `GET/PUT /families/{id}`).

use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};

use api::{
    auth::{AuthClaims, UserIdentity},
    errors::{ApiJson, domain_error_response},
    response::{created, ok},
};
use domain::{
    entities::Family,
    errors::DomainError,
    ports::{EventPublisher, FamilyRepository, TokenScope},
    value_objects::{CurrencyCode, MemberDisplayName, FamilyId, FamilyName, Role},
};

use super::{load_family_or_not_found, parse_and_verify_path_family_id, publish_best_effort};
use crate::{
    app_state::AppState,
    dto::{
        CreateFamilyRequest, CreateFamilyResponse, FamilyResponse, ListUserFamiliesResponse,
        UpdateFamilyRequest, UserFamilySummaryDto,
    },
};

/// `POST /families` — creates a new family workspace with the caller as `Owner`.
pub async fn post_families<R, P>(
    State(state): State<AppState<R, P>>,
    identity: UserIdentity,
    ApiJson(req): ApiJson<CreateFamilyRequest>,
) -> Response
where
    R: FamilyRepository + Send + Sync + 'static,
    P: EventPublisher + Send + Sync + 'static,
{
    match create_family_workflow(&state, &identity, req).await {
        Ok(resp) => created(resp).into_response(),
        Err(err) => domain_error_response(err),
    }
}

/// `GET /profile/families` — lists active families where the caller is an active member.
pub async fn get_profile_families<R, P>(
    State(state): State<AppState<R, P>>,
    identity: UserIdentity,
) -> Response
where
    R: FamilyRepository + Send + Sync + 'static,
    P: Send + Sync + 'static,
{
    match state.repo.list_for_user(identity.user_id).await {
        Ok(summaries) => {
            let items = summaries
                .into_iter()
                .map(UserFamilySummaryDto::from)
                .collect();
            ok(ListUserFamiliesResponse { items }).into_response()
        }
        Err(err) => domain_error_response(err),
    }
}

/// `GET /families/{id}` — returns the active family workspace profile.
pub async fn get_family<R, P>(
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
    match get_family_workflow(&*state.repo, claims.family_id).await {
        Ok(resp) => ok(resp).into_response(),
        Err(err) => domain_error_response(err),
    }
}

/// `PUT /families/{id}` — updates family name and home display currency (`Role::Owner` only).
pub async fn put_family<R, P>(
    State(state): State<AppState<R, P>>,
    claims: AuthClaims,
    Path(raw_id): Path<String>,
    ApiJson(req): ApiJson<UpdateFamilyRequest>,
) -> Response
where
    R: FamilyRepository + Send + Sync + 'static,
    P: Send + Sync + 'static,
{
    if let Err(err) = parse_and_verify_path_family_id(&raw_id, claims.family_id) {
        return err.into_response();
    }
    match update_family_workflow(&*state.repo, &claims, req).await {
        Ok(resp) => ok(resp).into_response(),
        Err(err) => domain_error_response(err),
    }
}

async fn create_family_workflow<R, P>(
    state: &AppState<R, P>,
    identity: &UserIdentity,
    req: CreateFamilyRequest,
) -> Result<CreateFamilyResponse, DomainError>
where
    R: FamilyRepository,
    P: EventPublisher,
{
    let name = FamilyName::try_new(req.name)?;
    let home_currency = CurrencyCode::try_new(req.home_currency)?;
    let owner_display_name = MemberDisplayName::try_new(req.owner_display_name)?;

    let (family, events) =
        Family::create(name, home_currency, identity.user_id, owner_display_name)?;
    state.repo.insert_new(&family).await?;
    publish_best_effort(&*state.publisher, &events).await;
    build_create_family_response(&family)
}

fn build_create_family_response(family: &Family) -> Result<CreateFamilyResponse, DomainError> {
    let owner_member_id = family
        .active_members()
        .find(|member| member.role() == Role::Owner)
        .map(|member| member.id())
        .ok_or_else(|| DomainError::Internal {
            message: "Created family is missing an active Owner member".to_owned(),
        })?;

    Ok(CreateFamilyResponse {
        id: family.id(),
        name: family.name().clone(),
        home_currency: family.home_currency().clone(),
        owner_member_id,
        created_at: family.created_at(),
    })
}

async fn get_family_workflow<R: FamilyRepository>(
    repo: &R,
    family_id: FamilyId,
) -> Result<FamilyResponse, DomainError> {
    let family = load_family_or_not_found(repo, family_id).await?;
    if family.is_deleted() {
        return Err(DomainError::FamilyDeleted);
    }
    Ok(FamilyResponse::from(&family))
}

async fn update_family_workflow<R: FamilyRepository>(
    repo: &R,
    claims: &AuthClaims,
    req: UpdateFamilyRequest,
) -> Result<FamilyResponse, DomainError> {
    let name = FamilyName::try_new(req.name)?;
    let home_currency = CurrencyCode::try_new(req.home_currency)?;
    let user_id = claims.user_id;
    let updated = repo
        .update(claims.family_id, TokenScope::None, move |family| {
            // Cloned per attempt because OCC retries may re-execute this closure.
            family.update_profile(user_id, name.clone(), home_currency.clone())?;
            Ok((FamilyResponse::from(&*family), Vec::new()))
        })
        .await?;
    Ok(updated.value)
}
