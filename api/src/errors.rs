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
        E::AccountNotFound { .. } => (S::NOT_FOUND, "ACCOUNT_NOT_FOUND"),
        E::CategoryNotFound { .. } => (S::NOT_FOUND, "CATEGORY_NOT_FOUND"),
        E::TransactionNotFound { .. } => (S::NOT_FOUND, "TRANSACTION_NOT_FOUND"),
        E::FamilyDeleted => (S::GONE, "FAMILY_DELETED"),
        E::TokenExpired => (S::GONE, "TOKEN_EXPIRED"),
        E::AccountDeleted { .. } => (S::GONE, "ACCOUNT_DELETED"),
        E::CategoryDeleted { .. } => (S::GONE, "CATEGORY_DELETED"),
        E::TransactionDeleted { .. } => (S::GONE, "TRANSACTION_DELETED"),
        E::TokenAlreadyUsed => (S::CONFLICT, "TOKEN_ALREADY_USED"),
        E::MemberAlreadyExists { .. } => (S::CONFLICT, "MEMBER_ALREADY_EXISTS"),
        E::DuplicateOwner => (S::CONFLICT, "DUPLICATE_OWNER"),
        E::CannotRemoveOwner => (S::CONFLICT, "CANNOT_REMOVE_OWNER"),
        E::CannotModifySystemCategory { .. } => (S::CONFLICT, "CANNOT_MODIFY_SYSTEM_CATEGORY"),
        E::TransactionAlreadyAmended { .. } => (S::CONFLICT, "TRANSACTION_ALREADY_AMENDED"),
        E::IdempotencyConflict { .. } => (S::CONFLICT, "IDEMPOTENCY_CONFLICT"),
        E::Internal { .. } => (S::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        _ => validation_error_code(err),
    }
}

fn validation_error_code(err: &DomainError) -> (StatusCode, &'static str) {
    use DomainError as E;
    let code = match err {
        E::CurrencyMismatch { .. } => "CURRENCY_MISMATCH",
        E::NegativeAmount { .. } => "NEGATIVE_AMOUNT",
        E::NonPositiveAmount { .. } => "NON_POSITIVE_AMOUNT",
        E::ArithmeticOverflow { .. } => "ARITHMETIC_OVERFLOW",
        E::InvalidCurrencyCode { .. } => "INVALID_CURRENCY_CODE",
        E::InvalidYearMonth { .. } => "INVALID_YEAR_MONTH",
        E::InvalidHexColor { .. } => "INVALID_HEX_COLOR",
        E::InvalidFamilyName { .. } => "INVALID_FAMILY_NAME",
        E::InvalidDisplayName { .. } => "INVALID_DISPLAY_NAME",
        E::InvalidEmail { .. } => "INVALID_EMAIL",
        E::InvalidRelationship { .. } => "INVALID_RELATIONSHIP",
        E::InvalidInviteTtl { .. } => "INVALID_INVITE_TTL",
        E::InvalidAccountName { .. } => "INVALID_ACCOUNT_NAME",
        E::InvalidBankName { .. } => "INVALID_BANK_NAME",
        E::InvalidLastFour { .. } => "INVALID_LAST_FOUR",
        E::CreditLimitNotAllowed { .. } => "CREDIT_LIMIT_NOT_ALLOWED",
        E::InvalidCategoryName { .. } => "INVALID_CATEGORY_NAME",
        E::InvalidCategoryIcon { .. } => "INVALID_CATEGORY_ICON",
        E::InvalidTag { .. } => "INVALID_TAG",
        E::TooManyTags { .. } => "TOO_MANY_TAGS",
        E::InvalidDescription { .. } => "INVALID_DESCRIPTION",
        E::CategoryKindMismatch { .. } => "CATEGORY_KIND_MISMATCH",
        E::CannotAmendReversal => "CANNOT_AMEND_REVERSAL",
        E::SameAccountTransfer { .. } => "SAME_ACCOUNT_TRANSFER",
        E::InvalidCashWithdrawalAccounts => "INVALID_CASH_WITHDRAWAL_ACCOUNTS",
        E::CrossCurrencyDestinationRequired => "CROSS_CURRENCY_DESTINATION_REQUIRED",
        E::SameCurrencyDestinationForbidden => "SAME_CURRENCY_DESTINATION_FORBIDDEN",
        E::InvalidPaymentSplit { .. } => "INVALID_PAYMENT_SPLIT",
        _ => "INVARIANT_VIOLATION",
    };
    (StatusCode::UNPROCESSABLE_ENTITY, code)
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        routing::post,
    };
    use domain::value_objects::{
        AccountId, AccountKind, CategoryId, CurrencyCode, FamilyId, MemberId, Money, Role,
        TransactionId, UserId,
    };
    use serde::Deserialize;
    use tower::ServiceExt;
    use uuid::Uuid;

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

    fn non_validation_error_cases() -> Vec<(DomainError, StatusCode, &'static str)> {
        let mut cases = family_non_validation_cases();
        cases.extend(sprint_3_account_category_error_cases());
        cases.extend(sprint_3_tx_idempotency_error_cases());
        cases
    }

    fn family_non_validation_cases() -> Vec<(DomainError, StatusCode, &'static str)> {
        use DomainError as E;
        use StatusCode as S;
        let (uid, fid, mid) = (UserId::new_v4(), FamilyId::new_v4(), MemberId::new_v4());
        vec![
            (
                E::InsufficientRole { role: Role::Child },
                S::FORBIDDEN,
                "INSUFFICIENT_ROLE",
            ),
            (
                E::UserNotInFamily { user_id: uid },
                S::FORBIDDEN,
                "USER_NOT_IN_FAMILY",
            ),
            (
                E::FamilyNotFound { family_id: fid },
                S::NOT_FOUND,
                "FAMILY_NOT_FOUND",
            ),
            (
                E::MemberNotFound { member_id: mid },
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
                E::MemberAlreadyExists { user_id: uid },
                S::CONFLICT,
                "MEMBER_ALREADY_EXISTS",
            ),
            (E::DuplicateOwner, S::CONFLICT, "DUPLICATE_OWNER"),
            (E::CannotRemoveOwner, S::CONFLICT, "CANNOT_REMOVE_OWNER"),
        ]
    }

    fn sprint_3_account_category_error_cases() -> Vec<(DomainError, StatusCode, &'static str)> {
        use DomainError as E;
        use StatusCode as S;
        let (aid, cid) = (AccountId::new_v4(), CategoryId::new_v4());
        vec![
            (E::AccountNotFound { account_id: aid }, S::NOT_FOUND, "ACCOUNT_NOT_FOUND"),
            (E::AccountDeleted { account_id: aid }, S::GONE, "ACCOUNT_DELETED"),
            (E::CategoryNotFound { category_id: cid }, S::NOT_FOUND, "CATEGORY_NOT_FOUND"),
            (E::CategoryDeleted { category_id: cid }, S::GONE, "CATEGORY_DELETED"),
            (
                E::CannotModifySystemCategory { category_id: cid },
                S::CONFLICT,
                "CANNOT_MODIFY_SYSTEM_CATEGORY",
            ),
        ]
    }

    fn sprint_3_tx_idempotency_error_cases() -> Vec<(DomainError, StatusCode, &'static str)> {
        use DomainError as E;
        use StatusCode as S;
        let tid = TransactionId::now_v7();
        vec![
            (
                E::TransactionNotFound { transaction_id: tid },
                S::NOT_FOUND,
                "TRANSACTION_NOT_FOUND",
            ),
            (
                E::TransactionDeleted { transaction_id: tid },
                S::GONE,
                "TRANSACTION_DELETED",
            ),
            (
                E::TransactionAlreadyAmended { transaction_id: tid },
                S::CONFLICT,
                "TRANSACTION_ALREADY_AMENDED",
            ),
            (
                E::IdempotencyConflict { idempotency_key: Uuid::new_v4() },
                S::CONFLICT,
                "IDEMPOTENCY_CONFLICT",
            ),
        ]
    }

    fn validation_error_cases() -> Vec<(DomainError, &'static str)> {
        let mut cases = base_validation_cases();
        cases.extend(sprint_3_account_category_validation_cases());
        cases.extend(sprint_3_ledger_validation_cases());
        cases
    }

    fn base_validation_cases() -> Vec<(DomainError, &'static str)> {
        use DomainError as E;
        let expected = CurrencyCode::try_new("USD").unwrap();
        let actual = CurrencyCode::try_new("EUR").unwrap();
        let neg = Money::from_str_parts("-1.00", "USD").unwrap_err();
        let r = |msg: &str| msg.to_owned();
        vec![
            (E::CurrencyMismatch { expected, actual }, "CURRENCY_MISMATCH"),
            (neg, "NEGATIVE_AMOUNT"),
            (
                E::NonPositiveAmount {
                    amount: Money::from_str_parts("0", "USD").unwrap().amount(),
                },
                "NON_POSITIVE_AMOUNT",
            ),
            (E::ArithmeticOverflow { operation: "money addition" }, "ARITHMETIC_OVERFLOW"),
            (E::InvalidCurrencyCode { reason: r("3 chars") }, "INVALID_CURRENCY_CODE"),
            (E::InvalidEmail { reason: r("bad") }, "INVALID_EMAIL"),
            (E::InvalidRelationship { reason: r("empty") }, "INVALID_RELATIONSHIP"),
            (E::InvalidInviteTtl { reason: r("<= 0") }, "INVALID_INVITE_TTL"),
        ]
    }

    fn sprint_3_account_category_validation_cases() -> Vec<(DomainError, &'static str)> {
        use DomainError as E;
        let r = |msg: &str| msg.to_owned();
        vec![
            (E::InvalidYearMonth { reason: r("bad") }, "INVALID_YEAR_MONTH"),
            (E::InvalidHexColor { reason: r("bad") }, "INVALID_HEX_COLOR"),
            (E::InvalidAccountName { reason: r("bad") }, "INVALID_ACCOUNT_NAME"),
            (E::InvalidBankName { reason: r("bad") }, "INVALID_BANK_NAME"),
            (E::InvalidLastFour { reason: r("bad") }, "INVALID_LAST_FOUR"),
            (E::CreditLimitNotAllowed { kind: AccountKind::Cash }, "CREDIT_LIMIT_NOT_ALLOWED"),
            (E::InvalidCategoryName { reason: r("bad") }, "INVALID_CATEGORY_NAME"),
            (E::InvalidCategoryIcon { reason: r("bad") }, "INVALID_CATEGORY_ICON"),
        ]
    }

    fn sprint_3_ledger_validation_cases() -> Vec<(DomainError, &'static str)> {
        use DomainError as E;
        let r = |msg: &str| msg.to_owned();
        vec![
            (E::InvalidTag { reason: r("bad") }, "INVALID_TAG"),
            (E::TooManyTags { max: 20 }, "TOO_MANY_TAGS"),
            (E::InvalidDescription { reason: r("bad") }, "INVALID_DESCRIPTION"),
            (
                E::CategoryKindMismatch { expected: "expense".into(), actual: "income".into() },
                "CATEGORY_KIND_MISMATCH",
            ),
            (E::CannotAmendReversal, "CANNOT_AMEND_REVERSAL"),
            (E::SameAccountTransfer { account_id: AccountId::new_v4() }, "SAME_ACCOUNT_TRANSFER"),
            (E::InvalidCashWithdrawalAccounts, "INVALID_CASH_WITHDRAWAL_ACCOUNTS"),
            (E::CrossCurrencyDestinationRequired, "CROSS_CURRENCY_DESTINATION_REQUIRED"),
            (E::SameCurrencyDestinationForbidden, "SAME_CURRENCY_DESTINATION_FORBIDDEN"),
        ]
    }

    fn api_json_test_app() -> Router {
        async fn echo(ApiJson(_body): ApiJson<TestPayload>) -> StatusCode {
            StatusCode::OK
        }
        Router::new().route("/", post(echo))
    }
}
