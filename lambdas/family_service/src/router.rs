//! Axum router wiring for `family_service`.

use std::sync::Arc;

use axum::{
    Router, middleware,
    routing::{MethodRouter, delete, get, patch, post, put},
};

use api::auth::{FamilyAuthState, family_auth_middleware, lambda_identity_middleware};
use domain::{
    ports::{EventPublisher, FamilyRepository, MembershipReader},
    value_objects::DomainAction,
};

use crate::{app_state::AppState, handlers};

/// Builds the `family_service` Axum [`Router`] for all Identity & Family endpoints.
pub fn build_router<R, P>(state: AppState<R, P>) -> Router
where
    R: FamilyRepository + MembershipReader + Send + Sync + 'static,
    P: EventPublisher + Send + Sync + 'static,
{
    let router = Router::new()
        .route("/families", post(handlers::post_families::<R, P>))
        .route(
            "/profile/families",
            get(handlers::get_profile_families::<R, P>),
        )
        .route(
            "/families/{id}",
            with_family_auth(
                &state,
                DomainAction::ViewFamilyWorkspace,
                get(handlers::get_family::<R, P>),
            ),
        )
        .route(
            "/families/{id}",
            with_family_auth(
                &state,
                DomainAction::ManageFamily,
                put(handlers::put_family::<R, P>),
            ),
        );

    let router = add_invite_and_roster_routes(router, &state);
    add_member_governance_routes(router, &state)
        .layer(middleware::from_fn(lambda_identity_middleware))
        .with_state(state)
}

fn add_invite_and_roster_routes<R, P>(
    router: Router<AppState<R, P>>,
    state: &AppState<R, P>,
) -> Router<AppState<R, P>>
where
    R: FamilyRepository + MembershipReader + Send + Sync + 'static,
    P: EventPublisher + Send + Sync + 'static,
{
    router
        .route(
            "/families/{id}/invites",
            with_family_auth(
                state,
                DomainAction::ManageFamily,
                post(handlers::post_family_invite::<R, P>),
            ),
        )
        .route(
            "/families/{id}/members",
            post(handlers::post_family_member::<R, P>),
        )
        .route(
            "/families/{id}/members",
            with_family_auth(
                state,
                DomainAction::ViewFamilyWorkspace,
                get(handlers::get_family_members::<R, P>),
            ),
        )
}

fn add_member_governance_routes<R, P>(
    router: Router<AppState<R, P>>,
    state: &AppState<R, P>,
) -> Router<AppState<R, P>>
where
    R: FamilyRepository + MembershipReader + Send + Sync + 'static,
    P: EventPublisher + Send + Sync + 'static,
{
    router
        .route(
            "/families/{id}/members/{mid}/role",
            with_family_auth(
                state,
                DomainAction::ManageFamily,
                patch(handlers::patch_member_role::<R, P>),
            ),
        )
        .route(
            "/families/{id}/members/{mid}",
            with_family_auth(
                state,
                DomainAction::ManageFamily,
                delete(handlers::delete_member::<R, P>),
            ),
        )
}

fn with_family_auth<R, P>(
    state: &AppState<R, P>,
    required_action: DomainAction,
    method_router: MethodRouter<AppState<R, P>>,
) -> MethodRouter<AppState<R, P>>
where
    R: MembershipReader + Send + Sync + 'static,
    P: Send + Sync + 'static,
{
    let auth_state = FamilyAuthState {
        membership_reader: Arc::clone(&state.repo),
        required_action,
    };
    method_router.route_layer(middleware::from_fn_with_state(
        auth_state,
        family_auth_middleware::<R>,
    ))
}
