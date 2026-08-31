//! Standard JSON error responses.
//!
//! Maps domain errors and infrastructure errors to appropriate HTTP status codes.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use domain::errors::DomainError;
use serde::Serialize;

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
        DomainError::TokenExpired => (StatusCode::GONE, "TOKEN_EXPIRED"),
        DomainError::TokenAlreadyUsed => (StatusCode::CONFLICT, "TOKEN_ALREADY_USED"),
        DomainError::MemberNotFound { .. } => (StatusCode::NOT_FOUND, "MEMBER_NOT_FOUND"),
        _ => (StatusCode::UNPROCESSABLE_ENTITY, "DOMAIN_ERROR"),
    };

    (status, Json(ApiError::with_code(err.to_string(), code))).into_response()
}
