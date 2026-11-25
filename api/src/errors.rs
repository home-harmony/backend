//! Standard RFC 9457 Problem Details error responses (`application/problem+json`).
//!
//! Maps domain errors and auth errors to HTTP status codes with machine-readable
//! `code` identifiers so frontends can map `error.code` to localized copy.

use axum::{
    extract::{FromRequest, Request},
    http::{StatusCode, header},
    response::{IntoResponse, Json, Response},
};
use serde::{Serialize, de::DeserializeOwned};

use domain::errors::DomainError;

const PROBLEM_JSON_CONTENT_TYPE: &str = "application/problem+json";
const INTERNAL_ERROR_DETAIL: &str = "An internal server error occurred";

/// JSON extractor returning RFC 9457 `400 Bad Request` (`INVALID_JSON`) on parse failure.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiJson<T>(pub T);

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

impl<S, T> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError::problem(
                StatusCode::BAD_REQUEST,
                "INVALID_JSON",
                rejection.body_text(),
            )),
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
    if matches!(err, DomainError::Internal { .. }) {
        tracing::error!(error = %err, "Internal domain/port error");
        return ApiError::problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            INTERNAL_ERROR_DETAIL,
        )
        .into_response();
    }
    let (status, code) = domain_error_status_and_code(&err);
    ApiError::problem(status, code, err.to_string()).into_response()
}

fn domain_error_status_and_code(err: &DomainError) -> (StatusCode, &'static str) {
    use DomainError as E;
    use StatusCode as S;
    match err {
        E::InsufficientRole { .. } => (S::FORBIDDEN, "INSUFFICIENT_ROLE"),
        E::UserNotInFamily { .. } => (S::FORBIDDEN, "USER_NOT_IN_FAMILY"),
        E::FamilyNotFound { .. } => (S::NOT_FOUND, "FAMILY_NOT_FOUND"),
        E::MemberNotFound { .. } => (S::NOT_FOUND, "MEMBER_NOT_FOUND"),
        E::InviteTokenNotFound => (S::NOT_FOUND, "INVITE_TOKEN_NOT_FOUND"),
        E::FamilyDeleted => (S::GONE, "FAMILY_DELETED"),
        E::TokenExpired => (S::GONE, "TOKEN_EXPIRED"),
        E::TokenAlreadyUsed => (S::CONFLICT, "TOKEN_ALREADY_USED"),
        E::MemberAlreadyExists { .. } => (S::CONFLICT, "MEMBER_ALREADY_EXISTS"),
        E::DuplicateOwner => (S::CONFLICT, "DUPLICATE_OWNER"),
        E::CannotRemoveOwner => (S::CONFLICT, "CANNOT_REMOVE_OWNER"),
        E::Internal { .. } => (S::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        E::CurrencyMismatch { .. } => (S::UNPROCESSABLE_ENTITY, "CURRENCY_MISMATCH"),
        E::NegativeAmount { .. } => (S::UNPROCESSABLE_ENTITY, "NEGATIVE_AMOUNT"),
        E::ArithmeticOverflow { .. } => (S::UNPROCESSABLE_ENTITY, "ARITHMETIC_OVERFLOW"),
        E::InvalidCurrencyCode { .. } => (S::UNPROCESSABLE_ENTITY, "INVALID_CURRENCY_CODE"),
        E::InvalidFamilyName { .. } => (S::UNPROCESSABLE_ENTITY, "INVALID_FAMILY_NAME"),
        E::InvalidDisplayName { .. } => (S::UNPROCESSABLE_ENTITY, "INVALID_DISPLAY_NAME"),
        E::InvalidEmail { .. } => (S::UNPROCESSABLE_ENTITY, "INVALID_EMAIL"),
        E::InvalidRelationship { .. } => (S::UNPROCESSABLE_ENTITY, "INVALID_RELATIONSHIP"),
        E::InvalidInviteTtl { .. } => (S::UNPROCESSABLE_ENTITY, "INVALID_INVITE_TTL"),
        E::InvalidPaymentSplit { .. } => (S::UNPROCESSABLE_ENTITY, "INVALID_PAYMENT_SPLIT"),
        E::InvariantViolation { .. } => (S::UNPROCESSABLE_ENTITY, "INVARIANT_VIOLATION"),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        routing::post,
    };
    use domain::value_objects::{CurrencyCode, FamilyId, MemberId, Money, Role, UserId};
    use serde::Deserialize;
    use tower::ServiceExt;

    use super::*;

    #[derive(Deserialize)]
    struct TestPayload {
        #[allow(dead_code)]
        name: String,
    }

    #[tokio::test]
    async fn domain_errors_map_to_expected_http_statuses_and_codes() {
        for (err, expected_status, expected_code) in non_validation_error_cases() {
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
        for (err, expected_code) in validation_error_cases() {
            let (status, content_type, json) = read_problem(domain_error_response(err)).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(content_type.as_deref(), Some("application/problem+json"));
            assert_eq!(json["code"], expected_code);
        }
    }

    #[tokio::test]
    async fn internal_domain_error_maps_to_500_without_leaking_raw_message() {
        let err = DomainError::Internal {
            message: "secret postgres dsn password=hunter2".to_owned(),
        };
        let (status, content_type, json) = read_problem(domain_error_response(err)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(content_type.as_deref(), Some("application/problem+json"));
        assert_eq!(json["code"], "INTERNAL_ERROR");
        assert_eq!(json["status"], 500);
        assert_eq!(json["detail"], "An internal server error occurred");
        assert!(!json["detail"].as_str().unwrap().contains("hunter2"));
    }

    #[tokio::test]
    async fn api_json_rejects_malformed_and_missing_field_payloads_with_400_invalid_json() {
        let app = api_json_test_app();
        for payload in ["{not-json", "{}"] {
            let req = Request::builder()
                .method("POST")
                .uri("/")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(payload))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            let (status, ct, json) = read_problem(resp).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(ct.as_deref(), Some("application/problem+json"));
            assert_eq!(json["code"], "INVALID_JSON");
        }
    }

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

    fn non_validation_error_cases() -> [(DomainError, StatusCode, &'static str); 11] {
        use DomainError as E;
        use StatusCode as S;
        let (user_id, family_id, member_id) =
            (UserId::new_v4(), FamilyId::new_v4(), MemberId::new_v4());
        [
            (
                E::InsufficientRole { role: Role::Child },
                S::FORBIDDEN,
                "INSUFFICIENT_ROLE",
            ),
            (
                E::UserNotInFamily { user_id },
                S::FORBIDDEN,
                "USER_NOT_IN_FAMILY",
            ),
            (
                E::FamilyNotFound { family_id },
                S::NOT_FOUND,
                "FAMILY_NOT_FOUND",
            ),
            (
                E::MemberNotFound { member_id },
                S::NOT_FOUND,
                "MEMBER_NOT_FOUND",
            ),
            (
                E::InviteTokenNotFound,
                S::NOT_FOUND,
                "INVITE_TOKEN_NOT_FOUND",
            ),
            (E::FamilyDeleted, S::GONE, "FAMILY_DELETED"),
            (E::TokenExpired, S::GONE, "TOKEN_EXPIRED"),
            (E::TokenAlreadyUsed, S::CONFLICT, "TOKEN_ALREADY_USED"),
            (
                E::MemberAlreadyExists { user_id },
                S::CONFLICT,
                "MEMBER_ALREADY_EXISTS",
            ),
            (E::DuplicateOwner, S::CONFLICT, "DUPLICATE_OWNER"),
            (E::CannotRemoveOwner, S::CONFLICT, "CANNOT_REMOVE_OWNER"),
        ]
    }

    fn validation_error_cases() -> [(DomainError, &'static str); 7] {
        use DomainError as E;
        let expected = CurrencyCode::try_new("USD").unwrap();
        let actual = CurrencyCode::try_new("EUR").unwrap();
        let neg = Money::from_str_parts("-1.00", "USD").unwrap_err();
        let reason = |msg: &str| msg.to_owned();
        [
            (
                E::CurrencyMismatch { expected, actual },
                "CURRENCY_MISMATCH",
            ),
            (neg, "NEGATIVE_AMOUNT"),
            (
                E::ArithmeticOverflow {
                    operation: "money addition",
                },
                "ARITHMETIC_OVERFLOW",
            ),
            (
                E::InvalidCurrencyCode {
                    reason: reason("must be 3 chars"),
                },
                "INVALID_CURRENCY_CODE",
            ),
            (
                E::InvalidEmail {
                    reason: reason("invalid"),
                },
                "INVALID_EMAIL",
            ),
            (
                E::InvalidRelationship {
                    reason: reason("empty"),
                },
                "INVALID_RELATIONSHIP",
            ),
            (
                E::InvalidInviteTtl {
                    reason: reason("must be > 0"),
                },
                "INVALID_INVITE_TTL",
            ),
        ]
    }

    fn api_json_test_app() -> Router {
        async fn echo(ApiJson(_body): ApiJson<TestPayload>) -> StatusCode {
            StatusCode::OK
        }
        Router::new().route("/", post(echo))
    }
}
