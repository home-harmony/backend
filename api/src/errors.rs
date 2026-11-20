//! Standard JSON error responses.
//!
//! Maps domain errors and infrastructure errors to appropriate HTTP status codes.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::Serialize;

use domain::errors::DomainError;

/// A standard error response body returned by all API endpoints.
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ApiError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
            code: None,
        }
    }

    pub fn with_code(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: message.into(),
            code: Some(code.into()),
        }
    }
}

/// Maps a [`DomainError`] to an HTTP response.
///
/// Business rule violations → 422 Unprocessable Entity
/// Authorization violations → 403 Forbidden
pub fn domain_error_response(err: DomainError) -> Response {
    let (status, code) = match &err {
        DomainError::InsufficientRole { .. } => (StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE"),
        DomainError::UserNotInFamily { .. } => (StatusCode::FORBIDDEN, "USER_NOT_IN_FAMILY"),
        DomainError::TokenExpired => (StatusCode::GONE, "TOKEN_EXPIRED"),
        DomainError::TokenAlreadyUsed => (StatusCode::CONFLICT, "TOKEN_ALREADY_USED"),
        DomainError::MemberNotFound { .. } => (StatusCode::NOT_FOUND, "MEMBER_NOT_FOUND"),
        _ => (StatusCode::UNPROCESSABLE_ENTITY, "DOMAIN_ERROR"),
    };

    (status, Json(ApiError::with_code(err.to_string(), code))).into_response()
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use domain::value_objects::{MemberId, Role, UserId};

    #[test]
    fn domain_errors_map_to_expected_http_statuses() {
        assert_eq!(
            domain_error_response(DomainError::InsufficientRole { role: Role::Child }).status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            domain_error_response(DomainError::UserNotInFamily {
                user_id: UserId::new_v4()
            })
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            domain_error_response(DomainError::MemberNotFound {
                member_id: MemberId::new_v4()
            })
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            domain_error_response(DomainError::TokenExpired).status(),
            StatusCode::GONE
        );
        assert_eq!(
            domain_error_response(DomainError::TokenAlreadyUsed).status(),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn validation_domain_errors_map_to_unprocessable_entity() {
        assert_eq!(
            domain_error_response(DomainError::InvalidEmail {
                reason: "invalid".to_string()
            })
            .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            domain_error_response(DomainError::InvalidRelationship {
                reason: "empty".to_string()
            })
            .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            domain_error_response(DomainError::InvalidInviteTtl {
                reason: "must be > 0".to_string()
            })
            .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}
