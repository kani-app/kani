#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_get, build_test_app, create_admin, test_state};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn a_created_volume_is_listed_back_with_its_name() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let (manga_id, _) = common::seed_manga_with_chapter(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    let created = app
        .clone()
        .oneshot(common::authed_post(
            &format!("/rest/manga/{manga_id}/volumes"),
            &cookie,
            json!({ "name": "Volume 1", "volume_num": 1 }),
        ))
        .await
        .unwrap();
    assert!(
        created.status().is_success(),
        "creating a volume failed: {}",
        created.status()
    );

    let listed = app
        .oneshot(authed_get(
            &format!("/rest/manga/{manga_id}/volumes"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);

    let body = common::body_json(listed).await;
    let volumes = body
        .get("volumes")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array())
        .expect("the listing must be an array of volumes");
    assert_eq!(
        volumes.len(),
        1,
        "exactly the created volume must come back"
    );
    assert_eq!(volumes[0]["name"], "Volume 1");
    assert_eq!(volumes[0]["manga_id"], manga_id);
    // `time`'s default serde emits an array the browser cannot parse.
    assert!(
        volumes[0]["created_at"].is_string(),
        "created_at must serialise as an RFC 3339 string, got {}",
        volumes[0]["created_at"]
    );
}

#[tokio::test]
async fn volumes_of_one_manga_do_not_leak_into_another() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let (mine, _) = common::seed_manga_with_chapter(&state).await;
    let (theirs, _) = common::seed_manga_with_chapter(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    app.clone()
        .oneshot(common::authed_post(
            &format!("/rest/manga/{mine}/volumes"),
            &cookie,
            json!({ "name": "Only Mine", "volume_num": 1 }),
        ))
        .await
        .unwrap();

    let listed = app
        .oneshot(authed_get(
            &format!("/rest/manga/{theirs}/volumes"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);

    let body = common::body_json(listed).await;
    let volumes = body
        .get("volumes")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array())
        .expect("the listing must be an array of volumes");
    assert!(
        volumes.is_empty(),
        "a volume must not appear under a different manga, got {volumes:?}"
    );
}
