#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{build_test_app, create_admin, create_regular_user, test_state};
use serde_json::json;
use tower::ServiceExt;

fn post(
    path: &str,
    cookie: Option<&str>,
    body: serde_json::Value,
) -> axum::http::Request<axum::body::Body> {
    let mut b = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header(axum::http::header::CONTENT_TYPE, "application/json");
    if let Some(c) = cookie {
        b = b
            .header(axum::http::header::COOKIE, common::csrf_cookie(c))
            .header("X-CSRF-Token", common::csrf_token(c));
    }
    b.body(axum::body::Body::from(body.to_string())).unwrap()
}

async fn seed_manga(db: &sqlx::SqlitePool, source_name: &str, source_manga_id: &str) -> (i64, i64) {
    let source_id: i64 =
        sqlx::query_scalar("INSERT INTO sources (name, version) VALUES (?, '0.1') RETURNING id")
            .bind(source_name)
            .fetch_one(db)
            .await
            .unwrap();
    let manga_id: i64 = sqlx::query_scalar(
        "INSERT INTO manga (source_id, source_manga_id, name, status) \
         VALUES (?, ?, 'Vagabond', 0) RETURNING id",
    )
    .bind(source_id)
    .bind(source_manga_id)
    .fetch_one(db)
    .await
    .unwrap();
    (source_id, manga_id)
}

fn migrate_body(target_source_id: i64) -> serde_json::Value {
    json!({
        "target_source_id": target_source_id,
        "target_source_manga_id": "tgt-1",
        "keep_orphaned_downloads": false,
    })
}

#[tokio::test]
async fn migrating_is_accepted_and_answers_with_a_job_id() {
    let state = test_state().await;
    let db = state.service.db.clone();
    let (_, manga_id) = seed_manga(&db, "origin", "m1").await;
    let (target_source_id, _) = seed_manga(&db, "target", "m2").await;

    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let res = app
        .oneshot(post(
            &format!("/rest/manga/{manga_id}/migrate"),
            Some(&cookie),
            migrate_body(target_source_id),
        ))
        .await
        .unwrap();

    assert_eq!(
        res.status(),
        StatusCode::ACCEPTED,
        "a migration does network I/O against the target source, so it must not \
         block the request"
    );
    let body = common::body_json(res).await;
    assert!(
        body.get("job_id").and_then(|v| v.as_str()).is_some(),
        "the caller has nothing to poll without a job id: {body}"
    );
}

/// Seeds the durable running-job state inspected by the duplicate-submission guard.
async fn seed_pending_migration(db: &sqlx::SqlitePool, manga_id: i64) {
    sqlx::query(
        "INSERT INTO jobs (id, job_type, status, priority, description, params_json, created_at) \
         VALUES (?, 'migration', 'running', 1, 'seeded', ?, 0)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(json!({ "manga_id": manga_id }).to_string())
    .execute(db)
    .await
    .unwrap();
}

#[tokio::test]
async fn a_second_migration_of_the_same_manga_is_refused() {
    let state = test_state().await;
    let db = state.service.db.clone();
    let (_, manga_id) = seed_manga(&db, "origin", "m1").await;
    let (target_source_id, _) = seed_manga(&db, "target", "m2").await;
    seed_pending_migration(&db, manga_id).await;

    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let res = app
        .oneshot(post(
            &format!("/rest/manga/{manga_id}/migrate"),
            Some(&cookie),
            migrate_body(target_source_id),
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::CONFLICT,
        "a concurrent migration of the same series must be refused, not queued"
    );
}

#[tokio::test]
async fn an_in_flight_migration_does_not_block_a_different_series() {
    let state = test_state().await;
    let db = state.service.db.clone();
    let (_, busy_manga) = seed_manga(&db, "origin", "m1").await;
    let (_, other_manga) = seed_manga(&db, "other", "m2").await;
    let (target_source_id, _) = seed_manga(&db, "target", "m3").await;
    seed_pending_migration(&db, busy_manga).await;

    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let res = app
        .oneshot(post(
            &format!("/rest/manga/{other_manga}/migrate"),
            Some(&cookie),
            migrate_body(target_source_id),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn a_malformed_migration_body_is_rejected() {
    let state = test_state().await;
    let db = state.service.db.clone();
    let (_, manga_id) = seed_manga(&db, "origin", "m1").await;

    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    for path in [
        format!("/rest/manga/{manga_id}/migrate"),
        "/rest/migrations/match".to_string(),
        "/rest/migrations/bulk".to_string(),
        "/rest/migrations/status".to_string(),
    ] {
        let res = app
            .clone()
            .oneshot(post(
                &path,
                Some(&cookie),
                json!({ "target_source_id": "not a number" }),
            ))
            .await
            .unwrap();
        assert!(
            res.status().is_client_error(),
            "{path}: expected a 4xx, got {}",
            res.status()
        );
    }
}

#[tokio::test]
async fn matching_is_capped_per_request_so_one_call_cannot_queue_the_whole_library() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let items: Vec<_> = (1..=21).map(|id| json!({ "manga_id": id })).collect();
    let res = app
        .oneshot(post(
            "/rest/migrations/match",
            Some(&cookie),
            json!({ "target_source_id": 1, "items": items }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = common::body_json(res).await;
    assert!(
        body.to_string().contains("At most 20"),
        "the refusal must name the limit, not fail on something else: {body}"
    );
}

#[tokio::test]
async fn the_chapter_listing_can_ask_for_orphans() {
    let state = test_state().await;
    let db = state.service.db.clone();
    let (_, manga_id) = seed_manga(&db, "origin", "m1").await;

    sqlx::query(
        "INSERT INTO chapters (manga_id, source_chapter_id, chapter_number, language, name, is_orphaned) \
         VALUES (?, 'old-5', 5.0, 'en', 'Kept', 1)",
    )
    .bind(manga_id)
    .execute(&db)
    .await
    .unwrap();

    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let hidden = common::body_json(
        app.clone()
            .oneshot(common::authed_get(
                &format!("/rest/manga/{manga_id}/chapters"),
                &cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        hidden["chapters"].as_array().map(|a| a.len()),
        Some(0),
        "orphans stay out of the default listing"
    );

    let shown = common::body_json(
        app.oneshot(common::authed_get(
            &format!("/rest/manga/{manga_id}/chapters?filter_orphaned=true"),
            &cookie,
        ))
        .await
        .unwrap(),
    )
    .await;
    let rows = shown["chapters"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "the filter must reach them: {shown}");
    assert_eq!(
        rows[0]["is_orphaned"], true,
        "the row renders its Orphaned badge from this field"
    );
}

async fn seed_job(db: &sqlx::SqlitePool, job_type: &str, result: serde_json::Value) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO jobs (id, job_type, status, priority, description, result_json, created_at, completed_at) \
         VALUES (?, ?, 'completed', 1, 'seeded', ?, 0, 0)",
    )
    .bind(&id)
    .bind(job_type)
    .bind(result.to_string())
    .execute(db)
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn a_user_who_can_migrate_can_read_the_migration_result() {
    let state = test_state().await;
    let db = state.service.db.clone();
    let job_id = seed_job(
        &db,
        "migration",
        json!({ "chapters_matched": 7, "chapters_orphaned": 1, "chapters_new": 2, "chapters_kept": 0 }),
    )
    .await;

    let (u, p) = create_regular_user(&state, "reader").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let res = app
        .oneshot(post(
            "/rest/migrations/status",
            Some(&cookie),
            json!({ "job_ids": [job_id] }),
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "migrating needs library:manage, so reading the outcome must not need admin:jobs"
    );
    let body = common::body_json(res).await;
    assert_eq!(body[0]["id"], job_id, "{body}");
    assert_eq!(body[0]["status"], "completed", "{body}");
    assert_eq!(body[0]["result"]["chapters_matched"], 7, "{body}");
}

#[tokio::test]
async fn the_migration_status_route_does_not_reveal_other_jobs() {
    let state = test_state().await;
    let db = state.service.db.clone();
    let other = seed_job(&db, "backup", json!({ "path": "/secret/backup.zip" })).await;

    let (u, p) = create_regular_user(&state, "reader").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let res = app
        .oneshot(post(
            "/rest/migrations/status",
            Some(&cookie),
            json!({ "job_ids": [other] }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = common::body_json(res).await;
    assert_eq!(
        body,
        json!([]),
        "only migration jobs are readable here; anything else stays behind admin:jobs"
    );
}
