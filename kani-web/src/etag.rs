//! Conditional GET for the list endpoints a polling client walks.
//!
//! An integration that re-fetches the library every cycle mostly receives a
//! body it already has. `ETag` plus `If-None-Match` turns those into a 304.
//!
//! This is a bandwidth optimisation, not a query optimisation: the handler
//! still runs and still serialises, and the tag is derived from what it
//! produced. Skipping the query too would need a per-endpoint cheap version
//! stamp, which is a larger change and only worth making if profiling says the
//! query — not the transfer — is what hurts.
//!
//! Applied with `route_layer` on chosen routes rather than to the whole router:
//! buffering every GET would also buffer cover images and CBZ downloads, which
//! are the responses least worth holding in memory.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Responses larger than this are sent untagged. A body this size is a bulk
/// export rather than a list a client polls.
const MAX_TAGGED_BODY: usize = 4 * 1024 * 1024;

fn etag_for(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("\"{:016x}\"", hasher.finish())
}

/// True when any tag in an `If-None-Match` list matches, per RFC 9110 §13.1.2.
/// `*` matches any existing representation. The weak prefix is ignored on
/// comparison because a 304 only has to mean "semantically unchanged".
fn matches(header_value: &str, etag: &str) -> bool {
    header_value.split(',').map(str::trim).any(|candidate| {
        candidate == "*" || candidate.trim_start_matches("W/") == etag.trim_start_matches("W/")
    })
}

pub(crate) async fn etag_middleware(req: Request, next: Next) -> Response {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return next.run(req).await;
    }
    let if_none_match = req
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let res = next.run(req).await;
    if res.status() != StatusCode::OK {
        return res;
    }

    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_TAGGED_BODY).await else {
        // Too large to tag, and already consumed — the caller must not be left
        // with a truncated body, so this is an honest error rather than a
        // silent partial response.
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "error": "Response too large" })),
        )
            .into_response();
    };

    let etag = etag_for(&bytes);
    if let Ok(value) = HeaderValue::from_str(&etag) {
        parts.headers.insert(header::ETAG, value);
    }
    if !parts.headers.contains_key(header::CACHE_CONTROL) {
        parts.headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-cache"),
        );
    }

    if if_none_match.is_some_and(|v| matches(&v, &etag)) {
        parts.status = StatusCode::NOT_MODIFIED;
        parts.headers.remove(header::CONTENT_LENGTH);
        parts.headers.remove(header::CONTENT_TYPE);
        return Response::from_parts(parts, Body::empty());
    }

    Response::from_parts(parts, Body::from(bytes))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_same_body_tags_the_same_and_a_changed_one_does_not() {
        assert_eq!(etag_for(b"[{\"id\":1}]"), etag_for(b"[{\"id\":1}]"));
        assert_ne!(etag_for(b"[{\"id\":1}]"), etag_for(b"[{\"id\":2}]"));
    }

    #[test]
    fn a_tag_is_quoted() {
        let tag = etag_for(b"x");
        assert!(tag.starts_with('"') && tag.ends_with('"'), "got {tag}");
        assert!(
            HeaderValue::from_str(&tag).is_ok(),
            "a tag must be a legal header value"
        );
    }

    #[test]
    fn a_list_of_candidates_matches_on_any_member() {
        let tag = etag_for(b"body");
        assert!(matches(&format!("\"other\", {tag}"), &tag));
        assert!(!matches("\"other\", \"another\"", &tag));
    }

    #[test]
    fn a_star_matches_anything() {
        assert!(matches("*", &etag_for(b"whatever")));
    }

    #[test]
    fn weakness_does_not_defeat_a_match() {
        let tag = etag_for(b"body");
        assert!(
            matches(&format!("W/{tag}"), &tag),
            "a 304 only claims semantic equivalence, which a weak tag also asserts"
        );
    }
}
