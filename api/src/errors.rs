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
            [(header::CONTENT_TYPE, PROBLEM_JSON_CONTENT_TYPE)],
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
    match err {
        E::InsufficientRole { .. } => (StatusCode::FORBIDDEN, "INSUFFICIENT_ROLE"),
        E::UserNotInFamily { .. } => (StatusCode::FORBIDDEN, "USER_NOT_IN_FAMILY"),
        E::FamilyNotFound { .. } => (StatusCode::NOT_FOUND, "FAMILY_NOT_FOUND"),
        E::MemberNotFound { .. } => (StatusCode::NOT_FOUND, "MEMBER_NOT_FOUND"),
        E::InviteTokenNotFound => (StatusCode::NOT_FOUND, "INVITE_TOKEN_NOT_FOUND"),
        E::FamilyDeleted => (StatusCode::GONE, "FAMILY_DELETED"),
        E::TokenExpired => (StatusCode::GONE, "TOKEN_EXPIRED"),
        E::TokenAlreadyUsed => (StatusCode::CONFLICT, "TOKEN_ALREADY_USED"),
        E::MemberAlreadyExists { .. } => (StatusCode::CONFLICT, "MEMBER_ALREADY_EXISTS"),
        E::DuplicateOwner => (StatusCode::CONFLICT, "DUPLICATE_OWNER"),
        E::CannotRemoveOwner => (StatusCode::CONFLICT, "CANNOT_REMOVE_OWNER"),
        E::Internal { .. } => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        _ => (StatusCode::UNPROCESSABLE_ENTITY, validation_error_code(err)),
    }
}

fn validation_error_code(err: &DomainError) -> &'static str {
    use DomainError as E;
    match err {
        E::CurrencyMismatch { .. } => "CURRENCY_MISMATCH",
        E::NegativeAmount { .. } => "NEGATIVE_AMOUNT",
        E::ArithmeticOverflow { .. } => "ARITHMETIC_OVERFLOW",
        E::InvalidCurrencyCode { .. } => "INVALID_CURRENCY_CODE",
        E::InvalidFamilyName { .. } => "INVALID_FAMILY_NAME",
        E::InvalidDisplayName { .. } => "INVALID_DISPLAY_NAME",
        E::InvalidEmail { .. } => "INVALID_EMAIL",
        E::InvalidRelationship { .. } => "INVALID_RELATIONSHIP",
        E::InvalidInviteTtl { .. } => "INVALID_INVITE_TTL",
        E::InvalidPaymentSplit { .. } => "INVALID_PAYMENT_SPLIT",
        E::InvariantViolation { .. } => "INVARIANT_VIOLATION",
        E::InsufficientRole { .. }
        | E::UserNotInFamily { .. }
        | E::FamilyNotFound { .. }
        | E::MemberNotFound { .. }
        | E::InviteTokenNotFound
        | E::FamilyDeleted
        | E::TokenExpired
        | E::TokenAlreadyUsed
        | E::MemberAlreadyExists { .. }
        | E::DuplicateOwner
        | E::CannotRemoveOwner
        | E::Internal { .. } => "INTERNAL_ERROR",
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use domain::value_objects::{CurrencyCode, FamilyId, MemberId, Money, Role, UserId};

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
                DomainError::FamilyNotFound {
                    family_id: FamilyId::new_v4(),
                },
                StatusCode::NOT_FOUND,
                "FAMILY_NOT_FOUND",
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
        use axum::{Router, body::Body, routing::post};
        use serde::Deserialize;
        use tower::ServiceExt;

        #[derive(Deserialize)]
        struct Payload {
            #[allow(dead_code)]
            name: String,
        }

        async fn echo(ApiJson(_body): ApiJson<Payload>) -> StatusCode {
            StatusCode::OK
        }

        let app = Router::new().route("/", post(echo));

        let malformed_req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{not-json"))
            .unwrap();
        let malformed_resp = app.clone().oneshot(malformed_req).await.unwrap();
        let (status, ct, json) = read_problem(malformed_resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(ct.as_deref(), Some("application/problem+json"));
        assert_eq!(json["code"], "INVALID_JSON");

        let missing_field_req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let missing_resp = app.oneshot(missing_field_req).await.unwrap();
        let (status2, ct2, json2) = read_problem(missing_resp).await;
        assert_eq!(status2, StatusCode::BAD_REQUEST);
        assert_eq!(ct2.as_deref(), Some("application/problem+json"));
        assert_eq!(json2["code"], "INVALID_JSON");
    }
}
