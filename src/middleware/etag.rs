//! Conditional GETs for the app's reads.
//!
//! The Mac app refetches every screen on a timer, and most refetches return
//! exactly what it already has. A weak tag over the body lets it say so and
//! get an empty 304 back — the query still runs, but a megabyte of JSON does
//! not cross the wire or get parsed again.

use std::hash::{DefaultHasher, Hasher};

use axum::body::Body;
use axum::extract::Request;
use axum::http::header::{CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn etag(request: Request, next: Next) -> Response {
    // Only reads under /api/user: writes must always reach their body, and
    // /health is for probes that never cache.
    if request.method() != Method::GET || !request.uri().path().starts_with("/api/user/") {
        return next.run(request).await;
    }
    let sent = request.headers().get(IF_NONE_MATCH).cloned();
    let response = next.run(request).await;

    // An error is never "not modified", and anything that is not JSON may be
    // a stream this should not buffer.
    let is_json = response
        .headers()
        .get(CONTENT_TYPE)
        .is_some_and(|v| v.as_bytes().starts_with(b"application/json"));
    if response.status() != StatusCode::OK || !is_json {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    // `DefaultHasher::new()` is unkeyed, so the same body gets the same tag
    // across restarts. Weak, because this names the JSON, not the bytes of
    // any particular encoding of it.
    let mut hasher = DefaultHasher::new();
    hasher.write(&bytes);
    let tag = format!("W/\"{:016x}\"", hasher.finish());
    let tag = HeaderValue::from_str(&tag).expect("hex is a valid header");

    let matches = sent
        .as_ref()
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == tag || t.trim() == "*"));
    parts.headers.insert(ETAG, tag);
    if matches {
        parts.status = StatusCode::NOT_MODIFIED;
        parts.headers.remove(CONTENT_TYPE);
        parts.headers.remove(axum::http::header::CONTENT_LENGTH);
        return Response::from_parts(parts, Body::empty());
    }
    Response::from_parts(parts, Body::from(bytes))
}
