#![allow(clippy::unwrap_used)]

//! Authorization contract for `/rest/ui/themes`.
//!
//! The routes only require an authenticated user, because a user manages their
//! own themes. `theme:publish` is checked inside the handlers, so the tests that
//! matter most are the ones proving a regular user cannot reach the
//! instance-wide theme by any route: publishing it, editing it, or deleting it.

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{
    authed_get, authed_post, build_test_app, create_admin, create_regular_user, login, test_state,
};
use http_body_util::BodyExt as _;
use tower::ServiceExt;

fn theme_body(name: &str, instance_wide: bool) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "tokens": { "--color-accent": "#b93a24" },
        "instance_wide": instance_wide,
    })
}

fn put(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header("Cookie", common::csrf_cookie(cookie))
        .header("X-CSRF-Token", common::csrf_token(cookie))
        .body(Body::empty())
        .unwrap()
}

fn del(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("Cookie", common::csrf_cookie(cookie))
        .header("X-CSRF-Token", common::csrf_token(cookie))
        .body(Body::empty())
        .unwrap()
}

async fn json_of(res: axum::response::Response) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn list_themes_returns_200_for_an_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/ui/themes", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = json_of(res).await;
    assert!(body["themes"].is_array());
    assert!(body["active_id"].is_null(), "nothing is active yet");
}

#[tokio::test]
async fn an_unknown_token_is_rejected() {
    let (app, cookie) = common::admin_app().await;

    let body = serde_json::json!({
        "name": "Bad",
        "tokens": { "--evil": "#000" },
    });
    let res = app
        .oneshot(authed_post("/rest/ui/themes", &cookie, body))
        .await
        .unwrap();
    assert!(
        res.status().is_client_error(),
        "an unknown token must be refused, got {}",
        res.status()
    );
}

#[tokio::test]
async fn a_theme_can_be_created_activated_and_deleted() {
    let (app, cookie) = common::admin_app().await;

    let created = json_of(
        app.clone()
            .oneshot(authed_post(
                "/rest/ui/themes",
                &cookie,
                theme_body("Midnight", false),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["instance_wide"], false);

    let res = app
        .clone()
        .oneshot(put(&format!("/rest/ui/themes/{id}/activate"), &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let listed = json_of(
        app.clone()
            .oneshot(authed_get("/rest/ui/themes", &cookie))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed["active_id"], serde_json::json!(id));

    let res = app
        .clone()
        .oneshot(put("/rest/ui/themes/deactivate", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = app
        .clone()
        .oneshot(del(&format!("/rest/ui/themes/{id}"), &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let listed = json_of(
        app.oneshot(authed_get("/rest/ui/themes", &cookie))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed["themes"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn activating_an_unknown_theme_is_404() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(put("/rest/ui/themes/nope/activate", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_admin_can_publish_an_instance_wide_theme_and_others_see_it() {
    let state = test_state().await;
    let (au, ap) = create_admin(&state).await;
    let (bu, bp) = create_regular_user(&state, "bob").await;
    let app = build_test_app(state).await;

    let admin = login(&app, au, ap).await;
    let published = json_of(
        app.clone()
            .oneshot(authed_post(
                "/rest/ui/themes",
                &admin,
                theme_body("House Style", true),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(published["instance_wide"], true);

    let bob = login(&app, bu, bp).await;
    let listed = json_of(
        app.oneshot(authed_get("/rest/ui/themes", &bob))
            .await
            .unwrap(),
    )
    .await;
    let names: Vec<&str> = listed["themes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names, vec!["House Style"], "bob sees the published theme");
}

#[tokio::test]
async fn a_regular_user_cannot_delete_the_instance_theme() {
    let state = test_state().await;
    let (au, ap) = create_admin(&state).await;
    let (bu, bp) = create_regular_user(&state, "bob").await;
    let app = build_test_app(state).await;

    let admin = login(&app, au, ap).await;
    let published = json_of(
        app.clone()
            .oneshot(authed_post(
                "/rest/ui/themes",
                &admin,
                theme_body("House Style", true),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = published["id"].as_str().unwrap().to_string();

    let bob = login(&app, bu, bp).await;
    let res = app
        .clone()
        .oneshot(del(&format!("/rest/ui/themes/{id}"), &bob))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let listed = json_of(
        app.oneshot(authed_get("/rest/ui/themes", &admin))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed["themes"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_regular_user_cannot_edit_the_instance_theme_by_omitting_the_flag() {
    let state = test_state().await;
    let (au, ap) = create_admin(&state).await;
    let (bu, bp) = create_regular_user(&state, "bob").await;
    let app = build_test_app(state).await;

    let admin = login(&app, au, ap).await;
    let published = json_of(
        app.clone()
            .oneshot(authed_post(
                "/rest/ui/themes",
                &admin,
                theme_body("House Style", true),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = published["id"].as_str().unwrap().to_string();

    let bob = login(&app, bu, bp).await;
    let mut hijack = theme_body("Hijacked", false);
    hijack["id"] = serde_json::json!(id);
    let res = app
        .clone()
        .oneshot(authed_post("/rest/ui/themes", &bob, hijack))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let listed = json_of(
        app.oneshot(authed_get("/rest/ui/themes", &admin))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        listed["themes"][0]["name"], "House Style",
        "the published theme is unchanged"
    );
}

#[tokio::test]
async fn a_user_cannot_edit_another_users_theme() {
    let state = test_state().await;
    let (au, ap) = create_admin(&state).await;
    let (bu, bp) = create_regular_user(&state, "bob").await;
    let app = build_test_app(state).await;

    let alice = login(&app, au, ap).await;
    let mine = json_of(
        app.clone()
            .oneshot(authed_post(
                "/rest/ui/themes",
                &alice,
                theme_body("Alice's", false),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = mine["id"].as_str().unwrap().to_string();

    let bob = login(&app, bu, bp).await;
    let mut hijack = theme_body("Bob was here", false);
    hijack["id"] = serde_json::json!(id);
    let res = app
        .clone()
        .oneshot(authed_post("/rest/ui/themes", &bob, hijack))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let listed = json_of(
        app.oneshot(authed_get("/rest/ui/themes", &alice))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        listed["themes"][0]["name"], "Alice's",
        "alice's theme is unchanged"
    );
}

#[tokio::test]
async fn a_user_cannot_delete_another_users_theme() {
    let state = test_state().await;
    let (au, ap) = create_admin(&state).await;
    let (bu, bp) = create_regular_user(&state, "bob").await;
    let app = build_test_app(state).await;

    let alice = login(&app, au, ap).await;
    let mine = json_of(
        app.clone()
            .oneshot(authed_post(
                "/rest/ui/themes",
                &alice,
                theme_body("Alice's", false),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = mine["id"].as_str().unwrap().to_string();

    let bob = login(&app, bu, bp).await;
    let res = app
        .clone()
        .oneshot(del(&format!("/rest/ui/themes/{id}"), &bob))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let listed = json_of(
        app.oneshot(authed_get("/rest/ui/themes", &alice))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        listed["themes"].as_array().unwrap().len(),
        1,
        "alice's theme still exists"
    );
}

/// Every token the theme editor writes, exactly as `handleSave` in
/// `static/js/components/theme-editor.js` assembles it: `CORE_TOKENS` plus the
/// two accent values it derives. If the allowlist and `CORE_TOKENS` drift, the
/// editor starts failing to save and the only symptom is a 422.
#[tokio::test]
async fn the_editors_full_token_payload_is_accepted() {
    let (app, cookie) = common::admin_app().await;

    let core = [
        "--color-bg",
        "--color-surface",
        "--color-surface-2",
        "--color-surface-3",
        "--color-border",
        "--color-border-subtle",
        "--color-accent",
        "--color-text",
        "--color-text-muted",
        "--color-text-faint",
        "--color-success",
        "--color-warn",
        "--color-danger",
        "--color-accent-hover",
    ];
    let mut tokens = serde_json::Map::new();
    for name in core {
        tokens.insert(name.to_string(), serde_json::json!("#123456"));
    }
    tokens.insert(
        "--color-accent-dim".into(),
        serde_json::json!("rgba(18,52,86,0.15)"),
    );

    let res = app
        .clone()
        .oneshot(authed_post(
            "/rest/ui/themes",
            &cookie,
            serde_json::json!({ "name": "Full", "tokens": tokens }),
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "the editor's own payload must be accepted"
    );

    let saved = json_of(res).await;
    assert_eq!(
        saved["tokens"].as_object().unwrap().len(),
        core.len() + 1,
        "every token the editor sent comes back"
    );
}

/// `syncServerThemes` in `static/js/theme.js` adopts a local-only theme by
/// posting name + tokens + custom_css and nothing else — no `id`, no
/// `instance_wide`. That must create a theme owned by the caller, not one
/// published to everybody.
#[tokio::test]
async fn the_sync_upload_shape_creates_a_private_theme() {
    let state = test_state().await;
    let (au, ap) = create_admin(&state).await;
    let (bu, bp) = create_regular_user(&state, "bob").await;
    let app = build_test_app(state).await;

    let admin = login(&app, au, ap).await;
    let created = json_of(
        app.clone()
            .oneshot(authed_post(
                "/rest/ui/themes",
                &admin,
                serde_json::json!({
                    "name": "Adopted",
                    "tokens": { "--color-accent": "#b93a24" },
                    "custom_css": serde_json::Value::Null,
                }),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        created["instance_wide"], false,
        "an admin's sync upload must not publish for everyone by default"
    );

    let bob = login(&app, bu, bp).await;
    let listed = json_of(
        app.oneshot(authed_get("/rest/ui/themes", &bob))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        listed["themes"].as_array().unwrap().len(),
        0,
        "bob does not see another user's theme"
    );
}

#[tokio::test]
async fn posted_custom_css_comes_back_sanitised() {
    let (app, cookie) = common::admin_app().await;

    let mut body = theme_body("Styled", false);
    body["custom_css"] = serde_json::json!("@import url(evil.css); .btn { color: red }");
    app.clone()
        .oneshot(authed_post("/rest/ui/themes", &cookie, body))
        .await
        .unwrap();

    let listed = json_of(
        app.oneshot(authed_get("/rest/ui/themes", &cookie))
            .await
            .unwrap(),
    )
    .await;
    let css = listed["themes"][0]["custom_css"].as_str().unwrap();
    assert!(
        !css.contains("@import") && !css.contains("url("),
        "GET must not hand back what was posted: {css}"
    );
    assert!(css.contains("color: red"), "and the safe part survives");
}
