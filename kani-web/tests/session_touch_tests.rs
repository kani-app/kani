#![allow(clippy::unwrap_used)]

//! Session-sidecar upkeep: the `user_sessions` touch is debounced so a page of
//! images does not queue one write per request behind the single writer
//! connection, while per-request revocation stays unconditional.

mod common;
use axum::http::StatusCode;
use common::{authed_get, build_test_app, create_admin, login, test_state};
use tower::ServiceExt;

#[tokio::test]
async fn a_second_request_in_the_window_does_not_rewrite_the_session_row() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, u, p).await;

    // Login creates the session but does not touch it, so the window opens here.
    let warm = app
        .clone()
        .oneshot(authed_get("/rest/library", &cookie))
        .await
        .unwrap();
    assert_eq!(warm.status(), StatusCode::OK);

    // A sentinel no `unixepoch()` write could produce, so any touch is visible.
    sqlx::query("UPDATE user_sessions SET last_seen_at = 0")
        .execute(&state.service.db)
        .await
        .unwrap();

    let res = app
        .clone()
        .oneshot(authed_get("/rest/library", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let last_seen: i64 = sqlx::query_scalar("SELECT last_seen_at FROM user_sessions")
        .fetch_one(&state.service.db)
        .await
        .unwrap();
    assert_eq!(
        last_seen, 0,
        "a request inside the debounce window rewrote the session row"
    );
}

#[tokio::test]
async fn a_revoked_session_is_refused_even_while_the_touch_is_debounced() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, u, p).await;

    // Opens the debounce window, so the revocation below is checked on a request
    // whose touch write is suppressed.
    let warm = app
        .clone()
        .oneshot(authed_get("/rest/library", &cookie))
        .await
        .unwrap();
    assert_eq!(warm.status(), StatusCode::OK);

    sqlx::query("UPDATE user_sessions SET revoked_at = unixepoch()")
        .execute(&state.service.db)
        .await
        .unwrap();

    let res = app
        .clone()
        .oneshot(authed_get("/rest/library", &cookie))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::UNAUTHORIZED,
        "debouncing the touch must not skip the revocation check"
    );
}
