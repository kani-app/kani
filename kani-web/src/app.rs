//! Reusable REST router assembly with session/auth middleware.
//!
//! `build_app` is the testable core of the HTTP layer: it wires up the
//! session store, axum_login auth layer, and REST routes without adding
//! production-only concerns like rate-limiting, compression, or static files.
//! Tests call it directly; `main.rs` calls it and adds the rest.

use axum::{Router, http::header, response::IntoResponse};
use axum_login::{
    AuthManagerLayerBuilder,
    tower_sessions::{SessionManagerLayer, cookie::SameSite},
};
use tower_sessions_sqlx_store::SqliteStore;

use crate::{auth::AuthBackend, rest, state::AppState};

const CHANGELOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../CHANGELOG.md"));

async fn serve_changelog() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        CHANGELOG,
    )
}

pub async fn build_app(state: AppState) -> Router {
    let touch_state = state.clone();
    let idem_state = state.clone();
    let csrf_state = state.clone();
    let session_store = SqliteStore::new(state.db.clone());
    session_store
        .migrate()
        .await
        .expect("session store migration failed");

    let secure_cookies = std::env::var("KANI_SECURE_COOKIES")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(secure_cookies)
        .with_http_only(true)
        .with_same_site(SameSite::Lax);

    let auth_backend = AuthBackend::new(state.db.clone());
    let auth_layer = AuthManagerLayerBuilder::new(auth_backend, session_layer).build();

    let router = Router::new()
        .nest("/rest", rest::routes(state))
        .route("/changelog.md", axum::routing::get(serve_changelog));

    // Shadowing keeps the release build free of a debug-only mutable binding.
    #[cfg(debug_assertions)]
    let router = {
        use utoipa::OpenApi;
        use utoipa_swagger_ui::SwaggerUi;
        router.merge(
            SwaggerUi::new("/api-docs")
                .url("/api-docs/openapi.json", crate::openapi::ApiDoc::openapi()),
        )
    };

    router
        .layer(axum::middleware::from_fn_with_state(
            idem_state,
            crate::idempotency::idempotency_middleware,
        ))
        .layer(axum::middleware::from_fn(crate::auth::auth_guard))
        .layer(axum::middleware::from_fn_with_state(
            touch_state,
            crate::session_touch::session_touch_middleware,
        ))
        .layer(auth_layer)
        // Outside the session layer, so the response it inspects already carries
        // any rotated session cookie.
        .layer(axum::middleware::from_fn_with_state(
            csrf_state,
            crate::csrf::csrf_middleware,
        ))
        .layer(tower_http::request_id::PropagateRequestIdLayer::x_request_id())
        .layer(tower_http::request_id::SetRequestIdLayer::x_request_id(
            crate::middleware::trace_id::UuidRequestId,
        ))
}
