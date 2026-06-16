//! Standard JSON response helpers.

use axum::{http::StatusCode, response::Json};
use serde::Serialize;

/// Returns a `201 Created` response with the given body.
pub fn created<T: Serialize>(body: T) -> (StatusCode, Json<T>) {
    (StatusCode::CREATED, Json(body))
}

/// Returns a `200 OK` response with the given body.
pub fn ok<T: Serialize>(body: T) -> (StatusCode, Json<T>) {
    (StatusCode::OK, Json(body))
}

/// Returns a `204 No Content` response.
pub fn no_content() -> StatusCode {
    StatusCode::NO_CONTENT
}
