//! HTTP handlers and shared workflow helpers for `family_service`.

mod families;
mod members;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use api::{auth::AuthError, errors::ApiError};
use domain::{
    entities::Family,
    errors::DomainError,
    events::DomainEvent,
    ports::{EventPublisher, FamilyRepository},
    value_objects::{FamilyId, MemberId},
};

pub use families::{get_family, get_profile_families, post_families, put_family};
pub use members::{
    delete_member, get_family_members, patch_member_role, post_family_invite, post_family_member,
};

fn parse_family_and_member_paths(
    raw_family_id: &str,
    raw_member_id: &str,
    claims_family_id: FamilyId,
) -> Result<MemberId, Box<Response>> {
    if let Err(err) = parse_and_verify_path_family_id(raw_family_id, claims_family_id) {
        return Err(Box::new(err.into_response()));
    }
    parse_path_member_id(raw_member_id)
}

fn parse_and_verify_path_family_id(
    raw_id: &str,
    claims_family_id: FamilyId,
) -> Result<FamilyId, AuthError> {
    let path_id = parse_path_family_id(raw_id)?;
    if path_id != claims_family_id {
        return Err(AuthError::FamilyIdMismatch);
    }
    Ok(path_id)
}

fn parse_path_family_id(raw_id: &str) -> Result<FamilyId, AuthError> {
    raw_id
        .parse::<FamilyId>()
        .map_err(|_| AuthError::InvalidFamilyId)
}

fn parse_path_member_id(raw_id: &str) -> Result<MemberId, Box<Response>> {
    raw_id.parse::<MemberId>().map_err(|_| {
        Box::new(
            ApiError::problem(
                StatusCode::BAD_REQUEST,
                "INVALID_MEMBER_ID",
                "Invalid member ID path parameter; expected UUID",
            )
            .into_response(),
        )
    })
}

async fn load_family_or_not_found<R: FamilyRepository>(
    repo: &R,
    family_id: FamilyId,
) -> Result<Family, DomainError> {
    repo.find_by_id(family_id)
        .await?
        .ok_or(DomainError::FamilyNotFound { family_id })
}

async fn publish_best_effort<P: EventPublisher>(publisher: &P, events: &[DomainEvent]) {
    if events.is_empty() {
        return;
    }
    if let Err(err) = publisher.publish(events).await {
        let detail_types: Vec<&str> = events.iter().map(DomainEvent::detail_type).collect();
        tracing::error!(
            error = %err,
            family_id = %events[0].family_id(),
            event_count = events.len(),
            ?detail_types,
            "Best-effort EventBridge publish failed after DB commit"
        );
    }
}
