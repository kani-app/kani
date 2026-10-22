#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_get, body_array, body_json};
use tower::ServiceExt;

#[tokio::test]
async fn list_trackers_returns_anilist_and_mal() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/trackers", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let trackers = body_array(res).await;
    assert_eq!(
        trackers.len(),
        2,
        "AniList and MyAnimeList must always be seeded"
    );
    let names: Vec<&str> = trackers.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"AniList"));
    assert!(names.contains(&"MyAnimeList"));
}

#[tokio::test]
async fn list_trackers_all_unconfigured_on_fresh_db() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/trackers", &cookie))
        .await
        .unwrap();

    let trackers = body_array(res).await;
    for tracker in &trackers {
        assert_eq!(
            tracker["configured"],
            serde_json::json!(false),
            "no tracker should be configured on a fresh DB"
        );
        assert_eq!(tracker["linked"], serde_json::json!(false));
    }
}

#[tokio::test]
async fn get_tracker_config_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let list_res = app
        .clone()
        .oneshot(authed_get("/rest/trackers", &cookie))
        .await
        .unwrap();
    let trackers = body_array(list_res).await;
    let anilist_id = trackers
        .iter()
        .find(|t| t["name"].as_str() == Some("AniList"))
        .and_then(|t| t["id"].as_i64())
        .expect("AniList tracker must exist");

    let res = app
        .oneshot(authed_get(
            &format!("/rest/trackers/{anilist_id}/config"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["secret_configured"], serde_json::json!(false));
}
