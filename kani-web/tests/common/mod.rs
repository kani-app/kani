#![allow(clippy::unwrap_used, dead_code)]

pub mod routes;

use axum::{Router, body::Body, http::Request};
use axum_login::{
    AuthManagerLayerBuilder,
    tower_sessions::{SessionManagerLayer, cookie::SameSite},
};
use dashmap::DashMap;
use http_body_util::BodyExt;
use kani_app::AppService;
use kani_web::{
    auth::AuthBackend, logging::RingBufferLayer, rate_limit::AuthRateLimiter, state::AppState,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64},
};
use tower::ServiceExt;
use tower_sessions_sqlx_store::SqliteStore;

pub async fn test_db() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("../migrations").run(&pool).await.unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

/// Must match the `csrf_secret` [`test_state`] installs, or the tokens the request
/// helpers build will not validate.
const TEST_CSRF_SECRET: [u8; 32] = [0u8; 32];

pub async fn test_state() -> AppState {
    let pool = test_db().await;
    let service = Arc::new(AppService::new_for_test(pool.clone()).await);
    let (_, log_handle) = RingBufferLayer::new(100);
    AppState {
        rate_limiter: Arc::new(AuthRateLimiter::new(pool, service.settings.clone())),
        csrf_secret: Arc::new(TEST_CSRF_SECRET),
        trusted_proxies: Arc::new(Default::default()),
        public_instance: false,
        service,
        proxy_secret: Arc::new([0u8; 32]),
        proxy_semaphores: moka::future::Cache::builder().max_capacity(100).build(),
        proxy_throttle: moka::future::Cache::builder().max_capacity(100).build(),
        proxy_coalesce: moka::future::Cache::builder().max_capacity(100).build(),
        proxy_bandwidth: Arc::new(DashMap::<String, Arc<AtomicU64>>::new()),
        proxy_config: kani_web::proxy::ProxyConfig::default(),
        boot_id: "test".to_string(),
        restart_requested: Arc::new(AtomicBool::new(false)),
        log_handle,
        idempotency: kani_web::idempotency::IdempotencyStore::new(),
        session_touch_seen: moka::future::Cache::builder()
            .time_to_live(kani_web::state::SESSION_TOUCH_WINDOW)
            .build(),
    }
}

/// Build a testable axum router. All REST routes are mounted under `/rest`,
/// matching the production path prefix expected by `auth_guard`.
pub async fn build_test_app(state: AppState) -> Router {
    kani_web::app::build_app(state).await
}

/// A running app with an administrator already signed in, returning the router
/// and that session's cookie. Four lines of identical preamble opened almost
/// every REST test; seeding fixtures first needs [`admin_app_with_state`].
pub async fn admin_app() -> (Router, String) {
    let (app, cookie, _) = admin_app_with_state().await;
    (app, cookie)
}

/// As [`admin_app`], but also hands back the state so a test can read or seed
/// the database around the request it is making.
pub async fn admin_app_with_state() -> (Router, String, AppState) {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, username, password).await;
    (app, cookie, state)
}

pub const ADMIN_USERNAME: &str = "admin";
pub const ADMIN_PASSWORD: &str = "Password1234!";

/// A second signed-in session for the same administrator, for tests about one
/// session affecting another.
pub async fn second_admin_session(app: &Router) -> String {
    login(app, ADMIN_USERNAME, ADMIN_PASSWORD).await
}

/// Create an admin user in the given state's DB and return (username, password).
pub async fn create_admin(state: &AppState) -> (&'static str, &'static str) {
    let backend = AuthBackend::new(state.db.clone());
    let user = backend
        .create_user(ADMIN_USERNAME, "admin@test.local", ADMIN_PASSWORD)
        .await
        .unwrap();
    backend.grant_role(user.id, "admin", None).await.unwrap();
    (ADMIN_USERNAME, ADMIN_PASSWORD)
}

/// A user holding no roles at all, so every permission check must refuse them.
/// `create_user` grants the default `user` role, which carries real permissions;
/// stripping it is what makes an authorisation test about authorisation.
pub async fn create_permissionless_user(
    state: &AppState,
    username: &'static str,
) -> (&'static str, &'static str) {
    let credentials = create_regular_user(state, username).await;
    sqlx::query("DELETE FROM user_roles WHERE user_id = (SELECT id FROM users WHERE username = ?)")
        .bind(username)
        .execute(&state.db)
        .await
        .unwrap();
    credentials
}

/// Create a standard (non-admin) user and return (username, password).
pub async fn create_regular_user(
    state: &AppState,
    username: &'static str,
) -> (&'static str, &'static str) {
    let backend = AuthBackend::new(state.db.clone());
    backend
        .create_user(
            username,
            &format!("{}@test.local", username),
            "Password1234!",
        )
        .await
        .unwrap();
    (username, "Password1234!")
}

/// POST /rest/auth/login and return the session cookie string.
pub async fn login(app: &Router, username: &str, password: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/rest/auth/login")
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({"username": username, "password": password}))
                .unwrap(),
        ))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        axum::http::StatusCode::OK,
        "login must succeed for user '{username}'"
    );

    // Keep only the name=value part (drop Secure;HttpOnly;Path=/ etc.)
    res.headers()
        .get("set-cookie")
        .expect("set-cookie header missing after login")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

pub fn get_req(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

pub fn delete_req(uri: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// The CSRF token a browser holding `cookie` would have been given. Pair it with
/// [`csrf_cookie`] on any hand-built state-changing request, so tests exercise the
/// CSRF layer rather than bypassing it.
#[allow(dead_code)]
pub fn csrf_token(cookie: &str) -> String {
    let session = cookie
        .split(';')
        .find_map(|part| part.trim().strip_prefix("id="))
        .unwrap_or("anonymous");
    kani_web::csrf::compute_token(session, &TEST_CSRF_SECRET)
}

/// `cookie` with the matching `kani_csrf` cookie appended, as one `Cookie` header.
#[allow(dead_code)]
pub fn csrf_cookie(cookie: &str) -> String {
    format!("{cookie}; kani_csrf={}", csrf_token(cookie))
}

fn authed(method: &str, uri: &str, cookie: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if matches!(method, "GET" | "HEAD" | "OPTIONS") {
        builder = builder.header("Cookie", cookie);
    } else {
        builder = builder
            .header("Cookie", csrf_cookie(cookie))
            .header("X-CSRF-Token", csrf_token(cookie));
    }
    match body {
        Some(value) => builder
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&value).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

pub fn authed_get(uri: &str, cookie: &str) -> Request<Body> {
    authed("GET", uri, cookie, None)
}

pub fn authed_post(uri: &str, cookie: &str, body: serde_json::Value) -> Request<Body> {
    authed("POST", uri, cookie, Some(body))
}

pub fn authed_put(uri: &str, cookie: &str, body: serde_json::Value) -> Request<Body> {
    authed("PUT", uri, cookie, Some(body))
}

pub fn authed_patch(uri: &str, cookie: &str, body: serde_json::Value) -> Request<Body> {
    authed("PATCH", uri, cookie, Some(body))
}

pub fn authed_delete(uri: &str, cookie: &str) -> Request<Body> {
    authed("DELETE", uri, cookie, None)
}

pub fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap()
}

pub fn put_json(uri: &str, cookie: &str, body: serde_json::Value) -> Request<Body> {
    authed("PUT", uri, cookie, Some(body))
}

/// Drain the response body as raw bytes.
#[allow(dead_code)]
pub async fn body_bytes(res: axum::response::Response) -> axum::body::Bytes {
    res.into_body().collect().await.unwrap().to_bytes()
}

/// Drain the response body and parse as JSON.
pub async fn body_json(res: axum::response::Response) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

/// Drain the response body and parse as a JSON array.
pub async fn body_array(res: axum::response::Response) -> Vec<serde_json::Value> {
    body_json(res).await.as_array().cloned().unwrap_or_default()
}

/// Build a testable axum router with both REST and OPDS routes mounted.
/// OPDS handles its own auth per-handler; the auth_guard exempts /opds paths.
pub async fn build_test_app_with_opds(state: AppState) -> Router {
    let session_store = SqliteStore::new(state.db.clone());
    session_store.migrate().await.unwrap();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_http_only(true)
        .with_same_site(SameSite::Lax);
    let auth_backend = AuthBackend::new(state.db.clone());
    let auth_layer = AuthManagerLayerBuilder::new(auth_backend, session_layer).build();

    Router::new()
        .nest("/rest", kani_web::rest::routes(state.clone()))
        .nest("/opds", kani_web::opds::routes(state.clone()))
        .layer(axum::middleware::from_fn(kani_web::auth::auth_guard))
        .layer(axum::middleware::from_fn_with_state(
            state,
            kani_web::session_touch::session_touch_middleware,
        ))
        .layer(auth_layer)
}

/// Like `build_test_app` but also mounts the image-proxy router (a separate
/// router in production, composed in `main.rs`), so tests can drive `/rest/image_proxy`.
#[allow(dead_code)]
pub async fn build_test_app_with_proxy(state: AppState) -> Router {
    let session_store = SqliteStore::new(state.db.clone());
    session_store.migrate().await.unwrap();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_http_only(true)
        .with_same_site(SameSite::Lax);
    let auth_backend = AuthBackend::new(state.db.clone());
    let auth_layer = AuthManagerLayerBuilder::new(auth_backend, session_layer).build();

    Router::new()
        .merge(Router::new().nest("/rest", kani_web::rest::routes(state.clone())))
        .merge(Router::new().nest("/rest", kani_web::rest::image_proxy_route(state.clone())))
        .layer(axum::middleware::from_fn(kani_web::auth::auth_guard))
        .layer(axum::middleware::from_fn_with_state(
            state,
            kani_web::session_touch::session_touch_middleware,
        ))
        .layer(auth_layer)
}

// DB row inserters are shared via kani-shared-test (identical across crates).
#[allow(unused_imports)]
pub use kani_shared_test::{insert_chapter, insert_manga, insert_source, insert_user};

/// A source, a manga under it, and one chapter, returning `(manga_id, chapter_id)`.
///
/// Names are unique per call because `sources.name` is unique, so a test can seed
/// twice to prove one manga's rows do not leak into another's.
pub async fn seed_manga_with_chapter(state: &AppState) -> (i64, i64) {
    let unique = uuid::Uuid::new_v4().to_string();
    let source_id = insert_source(&state.db, &format!("src-{unique}")).await;
    let manga_id = insert_manga(&state.db, source_id, &unique, "Manga").await;
    let chapter_id = insert_chapter(&state.db, manga_id, &format!("c-{unique}"), 1.0).await;
    (manga_id.into(), chapter_id.into())
}

/// Build a Basic-auth `Authorization` header value for the given credentials.
pub fn basic_auth(username: &str, password: &str) -> String {
    use base64::Engine as _;
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    format!("Basic {encoded}")
}
