#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use kani_shared_test::{insert_manga, insert_source};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn scan_all_library_returns_job_id() {
    let state = common::test_state().await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;

    let res = app
        .oneshot(common::authed_post(
            "/rest/library/scan-all",
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = common::body_json(res).await;
    assert!(
        body.get("job_id").and_then(|v| v.as_str()).is_some(),
        "scan-all must return a job_id string; got: {body}"
    );
}

#[tokio::test]
async fn refresh_manga_returns_job_id() {
    let state = common::test_state().await;
    let source_id = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, source_id, "m1", "Test Manga").await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;

    let res = app
        .oneshot(common::authed_post(
            &format!("/rest/manga/{manga_id}/refresh"),
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = common::body_json(res).await;
    assert!(
        body.get("job_id").and_then(|v| v.as_str()).is_some(),
        "refresh must return a job_id string; got: {body}"
    );
}

#[tokio::test]
async fn untrash_by_token_returns_4xx_for_missing_token_field() {
    let state = common::test_state().await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;

    let res = app
        .oneshot(common::authed_post(
            "/rest/manga/untrash",
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();

    assert!(
        res.status().is_client_error(),
        "missing token field should be 4xx; got {}",
        res.status()
    );
}

#[tokio::test]
async fn untrash_by_token_returns_4xx_for_invalid_token_format() {
    let state = common::test_state().await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;

    let res = app
        .oneshot(common::authed_post(
            "/rest/manga/untrash",
            &cookie,
            json!({ "token": "not-a-uuid" }),
        ))
        .await
        .unwrap();

    assert!(
        res.status().is_client_error(),
        "non-UUID token should be 4xx; got {}",
        res.status()
    );
}

#[tokio::test]
async fn untrash_by_token_returns_404_for_unknown_token() {
    let state = common::test_state().await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;

    let res = app
        .oneshot(common::authed_post(
            "/rest/manga/untrash",
            &cookie,
            json!({ "token": uuid::Uuid::new_v4() }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn untrash_by_token_round_trip_restores_manga() {
    let state = common::test_state().await;
    let source_id = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, source_id, "m1", "Undo Test Manga").await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;

    let del = app
        .clone()
        .oneshot(common::authed_delete(
            &format!("/rest/manga/{manga_id}"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::OK);
    let del_body = common::body_json(del).await;
    let token = del_body
        .get("undo_token")
        .and_then(|v| v.as_str())
        .expect("delete must return an undo_token")
        .to_string();

    let restore = app
        .oneshot(common::authed_post(
            "/rest/manga/untrash",
            &cookie,
            json!({ "token": token }),
        ))
        .await
        .unwrap();
    assert_eq!(restore.status(), StatusCode::OK);
}

#[tokio::test]
async fn notify_prefs_lists_only_the_muted_manga() {
    let state = common::test_state().await;
    let (u, p) = common::create_admin(&state).await;
    let db = state.db.clone();
    let src: i64 =
        sqlx::query_scalar("INSERT INTO sources (name, version) VALUES ('s','1') RETURNING id")
            .fetch_one(&db)
            .await
            .unwrap();
    let mut ids = Vec::new();
    for name in ["Muted", "Loud"] {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO manga (source_id, source_manga_id, name, status) \
             VALUES (?, ?, ?, 0) RETURNING id",
        )
        .bind(src)
        .bind(name)
        .bind(name)
        .fetch_one(&db)
        .await
        .unwrap();
        ids.push(id);
    }

    let app = common::build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    for (id, notify) in [(ids[0], false), (ids[1], true)] {
        let res = app
            .clone()
            .oneshot(common::authed_put(
                &format!("/rest/manga/{id}/tracking"),
                &cookie,
                serde_json::json!({ "notify_new_chapters": notify }),
            ))
            .await
            .unwrap();
        assert!(res.status().is_success(), "manga {id}: {}", res.status());
    }

    let res = app
        .oneshot(common::authed_get("/rest/me/notify-prefs", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = common::body_json(res).await;
    let muted: Vec<i64> = body["muted"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_i64())
        .collect();

    assert_eq!(
        muted,
        vec![ids[0]],
        "only the muted series belongs here — the client defaults to notifying, \
         so shipping the whole library would be wasteful and wrong"
    );
}
