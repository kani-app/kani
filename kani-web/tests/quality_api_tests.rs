#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_get, build_test_app, create_admin, create_regular_user, test_state};
use serde_json::json;
use tower::ServiceExt;

fn req(
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<serde_json::Value>,
) -> axum::http::Request<axum::body::Body> {
    let mut b = axum::http::Request::builder().method(method).uri(path);
    if let Some(c) = cookie {
        if matches!(method, "GET" | "HEAD" | "OPTIONS") {
            b = b.header(axum::http::header::COOKIE, c);
        } else {
            b = b
                .header(axum::http::header::COOKIE, common::csrf_cookie(c))
                .header("X-CSRF-Token", common::csrf_token(c));
        }
    }
    match body {
        Some(v) => {
            b = b.header(axum::http::header::CONTENT_TYPE, "application/json");
            b.body(axum::body::Body::from(v.to_string())).unwrap()
        }
        None => b.body(axum::body::Body::empty()).unwrap(),
    }
}

#[tokio::test]
async fn library_wide_upgrades_list_is_readable_by_a_viewer() {
    let state = test_state().await;
    let (u, p) = create_regular_user(&state, "gail").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let res = app
        .oneshot(authed_get("/rest/me/upgrades", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(common::body_json(res).await.is_array());
}

#[tokio::test]
async fn a_library_wide_upgrade_names_its_series_and_chapter() {
    let state = test_state().await;
    let db = state.service.db.clone();

    let source_id: i64 =
        sqlx::query_scalar("INSERT INTO sources (name, version) VALUES ('s', '0.1') RETURNING id")
            .fetch_one(&db)
            .await
            .unwrap();
    let manga_id: i64 = sqlx::query_scalar(
        "INSERT INTO manga (source_id, source_manga_id, name, status) \
         VALUES (?, 'm1', 'Blade of the Immortal', 0) RETURNING id",
    )
    .bind(source_id)
    .fetch_one(&db)
    .await
    .unwrap();

    let descriptor = json!({
        "candidates": [{
            "held_chapter_id": 1,
            "kind": "preferred_scanlator",
            "candidate_chapter_id": 2,
            "candidate_source_chapter_id": "ch-7",
            "candidate_scanlator": "Group A",
            "held_scanlator": "Group Z",
            "candidate_page_count": 20,
            "held_page_count": 18,
            "reason_key": "upgrade.reason.preferred_scanlator",
            "detected_at": 0
        }],
        "dismissed": []
    });
    sqlx::query(
        "INSERT INTO chapters (manga_id, source_chapter_id, chapter_number, language, name, upgrade_available) \
         VALUES (?, 'ch-7', 7.0, 'en', 'Cricket', ?)",
    )
    .bind(manga_id)
    .bind(descriptor.to_string())
    .execute(&db)
    .await
    .unwrap();

    let (u, p) = create_regular_user(&state, "gail").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let res = app
        .oneshot(authed_get("/rest/me/upgrades", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = common::body_json(res).await;
    let row = &body.as_array().unwrap()[0];

    assert_eq!(row["manga_id"], manga_id);
    assert_eq!(row["manga_title"], "Blade of the Immortal");
    assert_eq!(row["chapter_number"], 7.0);
    assert_eq!(row["chapter_name"], "Cricket");
    assert_eq!(row["candidate"]["candidate_scanlator"], "Group A");
}

#[tokio::test]
async fn applying_an_upgrade_is_a_library_manage_action() {
    let state = test_state().await;
    let (u, p) = create_regular_user(&state, "hank").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let anon = app
        .clone()
        .oneshot(req("POST", "/rest/chapters/1/upgrade", None, None))
        .await
        .unwrap();
    assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);

    let authed = app
        .oneshot(req("POST", "/rest/chapters/1/upgrade", Some(&cookie), None))
        .await
        .unwrap();
    assert_ne!(
        authed.status(),
        StatusCode::FORBIDDEN,
        "a user holding library:manage must not be refused"
    );
}

#[tokio::test]
async fn applying_an_upgrade_to_a_missing_chapter_is_a_404() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(req(
            "POST",
            "/rest/chapters/9999/upgrade",
            Some(&cookie),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn dismiss_reports_a_missing_chapter_rather_than_succeeding() {
    let (app, cookie) = common::admin_app().await;

    let anon = app
        .clone()
        .oneshot(req("POST", "/rest/chapters/1/upgrade/dismiss", None, None))
        .await
        .unwrap();
    assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);

    let res = app
        .oneshot(req(
            "POST",
            "/rest/chapters/9999/upgrade/dismiss",
            Some(&cookie),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "silently succeeding would tell the UI a candidate was dismissed when \
         nothing was recorded"
    );
}

#[tokio::test]
async fn auto_replace_toggle_round_trips() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let db = state.db.clone();
    let src: i64 =
        sqlx::query_scalar("INSERT INTO sources (name, version) VALUES ('s', '1') RETURNING id")
            .fetch_one(&db)
            .await
            .unwrap();
    let manga: i64 = sqlx::query_scalar(
        "INSERT INTO manga (source_id, source_manga_id, name, status) \
         VALUES (?, 'm', 'M', 0) RETURNING id",
    )
    .bind(src)
    .fetch_one(&db)
    .await
    .unwrap();

    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let anon = app
        .clone()
        .oneshot(req(
            "PUT",
            &format!("/rest/manga/{manga}/upgrade-auto-replace"),
            None,
            Some(json!({ "enabled": true })),
        ))
        .await
        .unwrap();
    assert_eq!(
        anon.status(),
        StatusCode::UNAUTHORIZED,
        "auto-replace rewrites files on every scan; it must never be anonymous"
    );

    let res = app
        .clone()
        .oneshot(req(
            "PUT",
            &format!("/rest/manga/{manga}/upgrade-auto-replace"),
            Some(&cookie),
            Some(json!({ "enabled": true })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let stored: i64 = sqlx::query_scalar("SELECT upgrade_auto_replace FROM manga WHERE id = ?")
        .bind(manga)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(stored, 1, "the toggle must actually persist");

    let details = app
        .oneshot(authed_get(&format!("/rest/manga/{manga}/details"), &cookie))
        .await
        .unwrap();
    assert_eq!(details.status(), StatusCode::OK);
    let body = common::body_json(details).await;
    assert_eq!(
        body["upgrade_auto_replace"],
        serde_json::Value::Bool(true),
        "the details projection must carry the toggle, or the checkbox cannot \
         initialise from it"
    );
}

#[tokio::test]
async fn the_details_projection_carries_the_rail_facts() {
    let state = test_state().await;
    let db = state.service.db.clone();

    let source_id: i64 =
        sqlx::query_scalar("INSERT INTO sources (name, version) VALUES ('s', '1') RETURNING id")
            .fetch_one(&db)
            .await
            .unwrap();
    let manga: i64 = sqlx::query_scalar(
        "INSERT INTO manga (source_id, source_manga_id, name, status) \
         VALUES (?, 'm1', 'Vagabond', 0) RETURNING id",
    )
    .bind(source_id)
    .fetch_one(&db)
    .await
    .unwrap();
    for n in 1..=3 {
        sqlx::query(
            "INSERT INTO chapters (manga_id, source_chapter_id, chapter_number, language, name) \
             VALUES (?, ?, ?, 'en', 'c')",
        )
        .bind(manga)
        .bind(format!("c{n}"))
        .bind(n as f64)
        .execute(&db)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO chapters (manga_id, source_chapter_id, chapter_number, language, name, is_orphaned) \
         VALUES (?, 'old', 99.0, 'en', 'kept', 1)",
    )
    .bind(manga)
    .execute(&db)
    .await
    .unwrap();

    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let body = common::body_json(
        app.oneshot(authed_get(&format!("/rest/manga/{manga}/details"), &cookie))
            .await
            .unwrap(),
    )
    .await;

    assert_eq!(
        body["chapter_count"], 3,
        "the rail states the series' chapters, and a migration's orphans are not among them"
    );

    let added = body["added_at"].as_str().unwrap_or_default();
    assert!(!added.is_empty(), "added_at must reach the client: {body}");
    assert!(
        time::OffsetDateTime::parse(added, &time::format_description::well_known::Rfc3339).is_ok(),
        "must be RFC 3339 — time's default serde emits an array `new Date()` cannot parse: {added}"
    );
}
