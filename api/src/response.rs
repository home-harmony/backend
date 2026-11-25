//! Standard JSON response helpers and HTTP security header layers.

use axum::{
    http::{HeaderValue, StatusCode, header},
    response::Json,
};
use serde::Serialize;
use tower_http::set_header::SetResponseHeaderLayer;

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

/// Returns a `tower_http` layer setting `X-Content-Type-Options: nosniff`.
pub fn nosniff_layer() -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    )
}

/// Returns a `tower_http` layer setting `Cache-Control: no-store` for sensitive financial APIs.
pub fn no_store_cache_layer() -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    )
}
