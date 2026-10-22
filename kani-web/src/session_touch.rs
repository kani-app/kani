//! Tower middleware that enforces per-session revocation and maintains the
//! `user_sessions` sidecar table.
//!
//! This middleware holds both the resolved user and tower-sessions id, allowing
//! it to enforce revocation before touching the session sidecar.

use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_login::AuthSession;

use crate::{auth::AuthBackend, state::AppState};

/// Middleware: if the request is authenticated, record a session touch.
/// Passes through unconditionally — errors are non-fatal.
pub async fn session_touch_middleware(
    State(state): State<AppState>,
    mut auth: AuthSession<AuthBackend>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if let Some(user) = &auth.user {
        // Extract the tower-sessions session ID from the cookie header so we can
        // correlate with the user_sessions table.
        let session_id = auth.session.id().map(|id| id.to_string());
        if let Some(sid) = session_id {
            let user_id = user.id;

            // Revoked means revoked. Destroy the underlying session so the
            // cookie cannot be replayed, rather than merely refusing this one
            // request.
            if state.service.is_session_revoked(&sid).await {
                let _ = auth.logout().await;
                return (
                    axum::http::StatusCode::UNAUTHORIZED,
                    "This session has been signed out",
                )
                    .into_response();
            }

            if state.session_touch_seen.get(&sid).await.is_none() {
                let ua = request
                    .headers()
                    .get("user-agent")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_owned());
                let peer = request
                    .extensions()
                    .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                    .map(|info| info.0.ip());
                let ip =
                    crate::client_ip::client_ip(request.headers(), peer, &state.trusted_proxies)
                        .map(|a| a.to_string());

                if let Err(e) = state
                    .service
                    .touch_session(&sid, user_id, ua.as_deref(), ip.as_deref())
                    .await
                {
                    tracing::debug!("session touch failed: {e}");
                }
                state.session_touch_seen.insert(sid, ()).await;
            }
        }
    }

    next.run(request).await
}
