//! Double-submit cookie CSRF protection middleware.
//!
//! On GET/HEAD/OPTIONS requests: sets a `kani_csrf` cookie (SameSite=Strict, HttpOnly=false
//! so JS can read it). On state-changing methods (POST/PUT/PATCH/DELETE): validates that the
//! `X-CSRF-Token` header matches the signed cookie value.
//!
//! Exemptions:
//! - Requests with a valid `Authorization: Bearer …` header (API-token auth provides its own
//!   CSRF protection; the token cannot be sent cross-origin by a browser).
//! - Routes under `/rest/auth/passkey/` (WebAuthn flows use their own challenge).
//! - The routes that establish a session in the first place, listed in [`SESSION_ENTRY_ROUTES`].
//!   A caller has no session to bind a token to until one of these succeeds. Forced-login CSRF
//!   remains possible in theory; the session cookie's `SameSite=Lax` is what blocks it.
//!
//! The CSRF token is `HMAC-SHA256(session_id, csrf_secret)` encoded as base64url (first 32 bytes).
//! It is stable within a session so SPAs can read it once on load and reuse it.

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;

const CSRF_COOKIE: &str = "kani_csrf";
const CSRF_HEADER: &str = "x-csrf-token";
/// tower-sessions' default cookie name, which the session layer does not override.
const SESSION_COOKIE: &str = "id";
/// Value bound into the token before a session exists, so anonymous callers still
/// get a usable double-submit pair.
const ANONYMOUS: &str = "anonymous";

/// Routes that mint a session. Requiring a session-bound token to reach them is
/// circular, so they validate nothing; the response still carries a fresh cookie.
const SESSION_ENTRY_ROUTES: &[&str] = &[
    "/rest/auth/login",
    "/rest/auth/register",
    "/rest/auth/setup",
    "/rest/auth/forgot_password",
    "/rest/auth/reset_password",
];

/// Mirrors the session layer's own `KANI_SECURE_COOKIES` reading, so the two
/// cookies do not disagree about whether the deployment is behind TLS.
static SECURE_COOKIES: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
    std::env::var("KANI_SECURE_COOKIES").is_ok_and(|v| v == "true" || v == "1")
});

/// Reads one cookie's value out of a `Cookie` or `Set-Cookie` header value.
fn cookie_value(raw: &str, name: &str) -> Option<String> {
    raw.split(';').find_map(|part| {
        let part = part.trim();
        let rest = part.strip_prefix(name)?.strip_prefix('=')?;
        Some(rest.to_owned())
    })
}

/// Compute the CSRF token for `session_id` using the application's `csrf_secret`.
pub fn compute_token(session_id: &str, csrf_secret: &[u8; 32]) -> String {
    let mut mac = HmacSha256::new_from_slice(csrf_secret).expect("HMAC accepts any key size");
    mac.update(session_id.as_bytes());
    let result = mac.finalize().into_bytes();
    URL_SAFE_NO_PAD.encode(&result[..32])
}

/// Tower middleware that enforces CSRF protection on state-changing routes.
pub async fn csrf_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();

    // Passkey routes use their own WebAuthn challenge mechanism.
    if path.starts_with("/rest/auth/passkey/") {
        return next.run(request).await;
    }

    // Bearer-authenticated requests are inherently CSRF-safe — the token cannot
    // be sent cross-origin by a browser.
    if let Some(auth) = request.headers().get(header::AUTHORIZATION)
        && auth.to_str().unwrap_or_default().starts_with("Bearer ")
    {
        return next.run(request).await;
    }

    let request_cookies = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_default();

    let is_read_only = matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS");
    let is_session_entry = SESSION_ENTRY_ROUTES.contains(&path.as_str());

    // Only a request carrying a session has ambient authority for a forged one to
    // abuse. Skipping the sessionless case also leaves `auth_guard` free to answer
    // 401 rather than this layer masking it with 403.
    let session_cookie = cookie_value(&request_cookies, SESSION_COOKIE);

    if !is_read_only
        && !is_session_entry
        && let Some(session) = session_cookie.as_deref()
    {
        let expected = compute_token(session, &state.csrf_secret);
        let presented = request
            .headers()
            .get(CSRF_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        // The cookie is checked as well as recomputed: a caller that cannot read
        // the cookie cannot echo it, which is the whole basis of double submit.
        let cookie_token = cookie_value(&request_cookies, CSRF_COOKIE);
        let matches = presented.as_deref() == Some(expected.as_str())
            && cookie_token.as_deref() == Some(expected.as_str());

        if !matches {
            return (
                StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({
                    "error": "csrf_token_invalid",
                    "message": "Missing or invalid CSRF token. Include the X-CSRF-Token header."
                })),
            )
                .into_response();
        }
    }

    let mut response = next.run(request).await;

    // Bind to the session the response leaves the caller holding. Logging in
    // rotates the session, and a token minted against the previous one would be
    // rejected on the caller's next write.
    let session_after = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| cookie_value(v, SESSION_COOKIE))
        .or(session_cookie)
        .unwrap_or_else(|| ANONYMOUS.to_string());

    let token = compute_token(&session_after, &state.csrf_secret);
    let secure = if *SECURE_COOKIES { "; Secure" } else { "" };
    let cookie = format!("{CSRF_COOKIE}={token}; Path=/; SameSite=Strict{secure}");
    if let Ok(val) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, val);
    }
    response
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const SECRET: [u8; 32] = [0xDE; 32];

    #[test]
    fn compute_token_is_stable() {
        let t1 = compute_token("session-abc", &SECRET);
        let t2 = compute_token("session-abc", &SECRET);
        assert_eq!(
            t1, t2,
            "token must be deterministic for same session + secret"
        );
    }

    #[test]
    fn different_sessions_produce_different_tokens() {
        let t1 = compute_token("session-aaa", &SECRET);
        let t2 = compute_token("session-bbb", &SECRET);
        assert_ne!(t1, t2);
    }

    #[test]
    fn different_secrets_produce_different_tokens() {
        let secret2 = [0xAB; 32];
        let t1 = compute_token("session-xyz", &SECRET);
        let t2 = compute_token("session-xyz", &secret2);
        assert_ne!(t1, t2);
    }

    #[test]
    fn token_is_base64url_no_padding() {
        let t = compute_token("test", &SECRET);
        assert!(!t.contains('+'), "base64url must not contain '+'");
        assert!(!t.contains('/'), "base64url must not contain '/'");
        assert!(!t.contains('='), "no-pad base64url must not contain '='");
    }
}
