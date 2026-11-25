//! Standard RFC 9457 Problem Details error responses (`application/problem+json`).
//!
//! Maps domain errors and auth errors to HTTP status codes with machine-readable
//! `code` identifiers so frontends can map `error.code` to localized copy.

use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Json, Response},
};
use serde::Serialize;

use domain::errors::DomainError;

const PROBLEM_JSON_CONTENT_TYPE: &str = "application/problem+json";

/// RFC 9457 Problem Details payload returned by all API error responses.
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    /// URI reference identifying the problem type (`urn:home-harmony:error:<code_lowercase>`).
    #[serde(rename = "type")]
    pub problem_type: String,
    /// Standard HTTP reason phrase for `status`.
    pub title: String,
    /// HTTP status code echoed in the body.
    pub status: u16,
    /// Developer diagnostic explanation (never primary end-user UI copy).
    pub detail: String,
    /// Stable machine-readable error code for frontend localization.
    pub code: String,
}

impl ApiError {
    /// Builds an RFC 9457 Problem Details error response payload.
    pub fn problem(status: StatusCode, code: impl Into<String>, detail: impl Into<String>) -> Self {
        let code = code.into();
        let problem_type = format!("urn:home-harmony:error:{}", code.to_ascii_lowercase());
        let title = status.canonical_reason().unwrap_or("Error").to_owned();
        Self {
            problem_type,
            title,
            status: status.as_u16(),
            detail: detail.into(),
            code,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (
            status,
            [
                (header::CONTENT_TYPE, PROBLEM_JSON_CONTENT_TYPE),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            Json(self),
        )
            .into_response()
    }
}

/// Maps every [`DomainError`] variant exhaustively to an RFC 9457 HTTP response.
pub fn domain_error_response(err: DomainError) -> Response {
    let (status, code) = domain_error_status_and_code(&err);
    ApiError::problem(status, code, err.to_string()).into_response()
}

fn domain_error_status_and_code(err: &DomainError) -> (StatusCode, &'static str) {
    match err {
        DomainError::InsufficientRole { .. } => (StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE"),
        DomainError::UserNotInFamily { .. } => (StatusCode::FORBIDDEN, "USER_NOT_IN_FAMILY"),
        DomainError::MemberNotFound { .. } => (StatusCode::NOT_FOUND, "MEMBER_NOT_FOUND"),
        DomainError::InviteTokenNotFound => (StatusCode::NOT_FOUND, "INVITE_TOKEN_NOT_FOUND"),
        DomainError::FamilyDeleted => (StatusCode::GONE, "FAMILY_DELETED"),
        DomainError::TokenExpired => (StatusCode::GONE, "TOKEN_EXPIRED"),
        DomainError::TokenAlreadyUsed => (StatusCode::CONFLICT, "TOKEN_ALREADY_USED"),
        DomainError::MemberAlreadyExists { .. } => (StatusCode::CONFLICT, "MEMBER_ALREADY_EXISTS"),
        DomainError::DuplicateOwner => (StatusCode::CONFLICT, "DUPLICATE_OWNER"),
        DomainError::CannotRemoveOwner => (StatusCode::CONFLICT, "CANNOT_REMOVE_OWNER"),
        DomainError::CurrencyMismatch { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "CURRENCY_MISMATCH")
        }
        DomainError::NegativeAmount { .. } => (StatusCode::UNPROCESSABLE_ENTITY, "NEGATIVE_AMOUNT"),
        DomainError::ArithmeticOverflow { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "ARITHMETIC_OVERFLOW")
        }
        DomainError::InvalidCurrencyCode { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "INVALID_CURRENCY_CODE")
        }
        DomainError::InvalidFamilyName { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "INVALID_FAMILY_NAME")
        }
        DomainError::InvalidDisplayName { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "INVALID_DISPLAY_NAME")
        }
        DomainError::InvalidEmail { .. } => (StatusCode::UNPROCESSABLE_ENTITY, "INVALID_EMAIL"),
        DomainError::InvalidRelationship { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "INVALID_RELATIONSHIP")
        }
        DomainError::InvalidInviteTtl { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "INVALID_INVITE_TTL")
        }
        DomainError::InvalidPaymentSplit { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "INVALID_PAYMENT_SPLIT")
        }
        DomainError::InvariantViolation { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "INVARIANT_VIOLATION")
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use domain::value_objects::{CurrencyCode, MemberId, Money, Role, UserId};

    async fn read_problem(resp: Response) -> (StatusCode, Option<String>, serde_json::Value) {
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let body = to_bytes(resp.into_body(), 16 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        (status, content_type, json)
    }

    #[tokio::test]
    async fn domain_errors_map_to_expected_http_statuses_and_codes() {
        let cases = [
            (
                DomainError::InsufficientRole { role: Role::Child },
                StatusCode::FORBIDDEN,
                "INSUFFICIENT_ROLE",
            ),
            (
                DomainError::UserNotInFamily {
                    user_id: UserId::new_v4(),
                },
                StatusCode::FORBIDDEN,
                "USER_NOT_IN_FAMILY",
            ),
            (
                DomainError::MemberNotFound {
                    member_id: MemberId::new_v4(),
                },
                StatusCode::NOT_FOUND,
                "MEMBER_NOT_FOUND",
            ),
            (
                DomainError::InviteTokenNotFound,
                StatusCode::NOT_FOUND,
                "INVITE_TOKEN_NOT_FOUND",
            ),
            (
                DomainError::FamilyDeleted,
                StatusCode::GONE,
                "FAMILY_DELETED",
            ),
            (DomainError::TokenExpired, StatusCode::GONE, "TOKEN_EXPIRED"),
            (
                DomainError::TokenAlreadyUsed,
                StatusCode::CONFLICT,
                "TOKEN_ALREADY_USED",
            ),
            (
                DomainError::MemberAlreadyExists {
                    user_id: UserId::new_v4(),
                },
                StatusCode::CONFLICT,
                "MEMBER_ALREADY_EXISTS",
            ),
            (
                DomainError::DuplicateOwner,
                StatusCode::CONFLICT,
                "DUPLICATE_OWNER",
            ),
            (
                DomainError::CannotRemoveOwner,
                StatusCode::CONFLICT,
                "CANNOT_REMOVE_OWNER",
            ),
        ];

        for (err, expected_status, expected_code) in cases {
            let (status, content_type, json) = read_problem(domain_error_response(err)).await;
            assert_eq!(status, expected_status);
            assert_eq!(content_type.as_deref(), Some("application/problem+json"));
            assert_eq!(json["status"], expected_status.as_u16());
            assert_eq!(json["code"], expected_code);
            assert_eq!(
                json["type"],
                format!(
                    "urn:home-harmony:error:{}",
                    expected_code.to_ascii_lowercase()
                )
            );
            assert!(json["title"].as_str().is_some_and(|s| !s.is_empty()));
            assert!(json["detail"].as_str().is_some_and(|s| !s.is_empty()));
        }
    }

    #[tokio::test]
    async fn validation_domain_errors_map_to_distinct_unprocessable_entity_codes() {
        let usd = CurrencyCode::try_new("USD").unwrap();
        let eur = CurrencyCode::try_new("EUR").unwrap();

        let cases = [
            (
                DomainError::CurrencyMismatch {
                    expected: usd,
                    actual: eur,
                },
                "CURRENCY_MISMATCH",
            ),
            (
                Money::from_str_parts("-1.00", "USD").unwrap_err(),
                "NEGATIVE_AMOUNT",
            ),
            (
                DomainError::ArithmeticOverflow {
                    operation: "money addition",
                },
                "ARITHMETIC_OVERFLOW",
            ),
            (
                DomainError::InvalidCurrencyCode {
                    reason: "must be 3 chars".to_string(),
                },
                "INVALID_CURRENCY_CODE",
            ),
            (
                DomainError::InvalidEmail {
                    reason: "invalid".to_string(),
                },
                "INVALID_EMAIL",
            ),
            (
                DomainError::InvalidRelationship {
                    reason: "empty".to_string(),
                },
                "INVALID_RELATIONSHIP",
            ),
            (
                DomainError::InvalidInviteTtl {
                    reason: "must be > 0".to_string(),
                },
                "INVALID_INVITE_TTL",
            ),
        ];

        for (err, expected_code) in cases {
            let (status, content_type, json) = read_problem(domain_error_response(err)).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(content_type.as_deref(), Some("application/problem+json"));
            assert_eq!(json["code"], expected_code);
        }
    }
}
