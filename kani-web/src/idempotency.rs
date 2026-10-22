//! Replay protection for programmatic writes.
//!
//! A bot that POSTs, times out, and retries must not perform the write twice.
//! Supplying an `Idempotency-Key` header makes the retry return the original
//! response instead. Absent the header the middleware is a no-op, so browser
//! traffic and existing clients are untouched.
//!
//! The record lives in memory: a restart forgets it, and the guarantee is
//! per-process. That matches how Kani is deployed (one process, one SQLite
//! file) and keeps a write off the hot path of every write.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderName, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{auth::AuthSession, state::AppState};

pub(crate) const IDEMPOTENCY_KEY: HeaderName = HeaderName::from_static("idempotency-key");
const REPLAYED: HeaderName = HeaderName::from_static("x-idempotent-replay");

/// How long a key is honoured. Long enough to cover a client's timeout and
/// backoff, short enough that the map cannot grow without bound.
const TTL: Duration = Duration::from_secs(600);
const MAX_ENTRIES: u64 = 10_000;

/// Bodies larger than this are streamed through untouched rather than buffered
/// to be fingerprinted. A multi-hundred-megabyte import is not the double-submit
/// case this exists for, and buffering one would cost more than it saves.
const MAX_REQUEST_BODY: usize = 1024 * 1024;
const MAX_RESPONSE_BODY: usize = 256 * 1024;
const MAX_KEY_LEN: usize = 255;

#[derive(Clone)]
struct Recorded {
    fingerprint: u64,
    status: StatusCode,
    content_type: Option<HeaderValue>,
    body: Bytes,
}

type Slot = Arc<tokio::sync::Mutex<Option<Recorded>>>;

#[derive(Clone)]
/// Per-process store coordinating and replaying keyed write responses.
/// Concurrent duplicates wait for the original request; server failures are not retained.
pub struct IdempotencyStore(moka::future::Cache<String, Slot>);

impl Default for IdempotencyStore {
    fn default() -> Self {
        Self(
            moka::future::Cache::builder()
                .max_capacity(MAX_ENTRIES)
                .time_to_live(TTL)
                .build(),
        )
    }
}

impl IdempotencyStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn hash_of(parts: &[&[u8]]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for p in parts {
        p.hash(&mut hasher);
    }
    hasher.finish()
}

/// Namespace for a key. Two callers using the same key string must never see
/// each other's response, so the caller's identity is part of the map key —
/// not just the header value.
fn caller_id(auth: &AuthSession, req: &Request) -> Option<String> {
    if let Some(bearer) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| !v.is_empty())
    {
        return Some(format!("t{}", hash_of(&[bearer.as_bytes()])));
    }
    auth.user.as_ref().map(|u| format!("u{}", u.id))
}

fn is_write(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn oversized(req: &Request) -> bool {
    req.headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .is_some_and(|n| n > MAX_REQUEST_BODY)
}

fn err(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(serde_json::json!({ "error": message }))).into_response()
}

fn replay(rec: &Recorded) -> Response {
    let mut res = Response::builder()
        .status(rec.status)
        .header(REPLAYED, HeaderValue::from_static("true"));
    if let Some(ct) = &rec.content_type {
        res = res.header(header::CONTENT_TYPE, ct);
    }
    res.body(Body::from(rec.body.clone()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

pub(crate) async fn idempotency_middleware(
    State(state): State<AppState>,
    auth: AuthSession,
    req: Request,
    next: Next,
) -> Response {
    let Some(raw_key) = req.headers().get(IDEMPOTENCY_KEY).cloned() else {
        return next.run(req).await;
    };
    if !is_write(req.method()) || oversized(&req) {
        return next.run(req).await;
    }
    let Ok(key) = raw_key.to_str() else {
        return err(StatusCode::BAD_REQUEST, "Idempotency-Key must be ASCII");
    };
    if key.is_empty() || key.len() > MAX_KEY_LEN {
        return err(
            StatusCode::BAD_REQUEST,
            "Idempotency-Key must be 1-255 characters",
        );
    }
    // An unauthenticated write has no namespace to file the key under, and is
    // about to be refused anyway.
    let Some(caller) = caller_id(&auth, &req) else {
        return next.run(req).await;
    };
    let map_key = format!("{caller}:{key}");

    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, MAX_REQUEST_BODY).await {
        Ok(b) => b,
        Err(_) => return err(StatusCode::PAYLOAD_TOO_LARGE, "Request body too large"),
    };
    let fingerprint = hash_of(&[
        parts.method.as_str().as_bytes(),
        parts.uri.path().as_bytes(),
        parts.uri.query().unwrap_or("").as_bytes(),
        &bytes,
    ]);

    let slot = state
        .idempotency
        .0
        .get_with(map_key.clone(), async { Slot::default() })
        .await;

    // Held for the duration of the request, so a concurrent duplicate is told to
    // wait rather than racing the original through the handler.
    let Ok(mut guard) = slot.try_lock() else {
        return (
            StatusCode::CONFLICT,
            [(header::RETRY_AFTER, "1")],
            axum::Json(serde_json::json!({
                "error": "A request with this Idempotency-Key is already in progress"
            })),
        )
            .into_response();
    };

    if let Some(rec) = guard.as_ref() {
        if rec.fingerprint != fingerprint {
            return err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Idempotency-Key was already used for a different request",
            );
        }
        return replay(rec);
    }

    let res = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;

    // A server error is not a settled outcome: forget the key so the client's
    // retry actually retries instead of replaying the failure.
    if res.status().is_server_error() {
        drop(guard);
        state.idempotency.0.invalidate(&map_key).await;
        return res;
    }

    let (res_parts, res_body) = res.into_parts();
    let Ok(res_bytes) = axum::body::to_bytes(res_body, MAX_RESPONSE_BODY).await else {
        drop(guard);
        state.idempotency.0.invalidate(&map_key).await;
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Response too large to record for idempotent replay",
        );
    };

    *guard = Some(Recorded {
        fingerprint,
        status: res_parts.status,
        content_type: res_parts.headers.get(header::CONTENT_TYPE).cloned(),
        body: res_bytes.clone(),
    });

    Response::from_parts(res_parts, Body::from(res_bytes))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_fingerprint_separates_different_requests() {
        let a = hash_of(&[b"POST", b"/rest/x", b"", b"{\"n\":1}"]);
        let b = hash_of(&[b"POST", b"/rest/x", b"", b"{\"n\":2}"]);
        let c = hash_of(&[b"POST", b"/rest/y", b"", b"{\"n\":1}"]);
        assert_ne!(a, b, "a different body is a different request");
        assert_ne!(a, c, "a different path is a different request");
    }

    #[test]
    fn only_writes_are_eligible() {
        assert!(is_write(&Method::POST));
        assert!(is_write(&Method::DELETE));
        assert!(
            !is_write(&Method::GET),
            "a GET is already idempotent and must not be recorded"
        );
    }

    #[test]
    fn a_large_declared_body_opts_out() {
        let big = Request::builder()
            .method(Method::POST)
            .uri("/rest/import")
            .header(header::CONTENT_LENGTH, (MAX_REQUEST_BODY + 1).to_string())
            .body(Body::empty())
            .unwrap();
        assert!(oversized(&big));

        let small = Request::builder()
            .method(Method::POST)
            .uri("/rest/x")
            .header(header::CONTENT_LENGTH, "10")
            .body(Body::empty())
            .unwrap();
        assert!(!oversized(&small));
    }

    #[test]
    fn two_callers_sharing_a_key_string_get_different_namespaces() {
        let a = hash_of(&[b"kani_aaa"]);
        let b = hash_of(&[b"kani_bbb"]);
        assert_ne!(
            format!("t{a}:key-1"),
            format!("t{b}:key-1"),
            "one caller must never replay another caller's response"
        );
    }

    #[test]
    fn a_replayed_response_is_labelled() {
        let rec = Recorded {
            fingerprint: 1,
            status: StatusCode::CREATED,
            content_type: Some(HeaderValue::from_static("application/json")),
            body: Bytes::from_static(b"{}"),
        };
        let res = replay(&rec);
        assert_eq!(res.status(), StatusCode::CREATED);
        assert_eq!(res.headers().get(REPLAYED).unwrap(), "true");
    }
}
