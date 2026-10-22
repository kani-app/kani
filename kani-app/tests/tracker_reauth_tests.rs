#![allow(clippy::unwrap_used)]

//! Tracker credential failures persist `needs_reauth` for both rejected refreshes
//! and authorization failures received before the token's recorded expiry.

mod common;
use common::{insert_user, test_service};

use kani_app::ids::UserId;
use kani_app::service::trackers::mal::MalTracker;
use kani_app::service::trackers::{ExternalTracker, get_access_token};
use kani_shared_test::origin::{Response, TestOrigin};
use sqlx::SqlitePool;

/// `TrackerRegistry::new` seeds the provider rows, so look the id up rather
/// than inserting a colliding one.
async fn tracker_id(db: &SqlitePool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT id FROM trackers WHERE name = 'MyAnimeList'")
        .fetch_one(db)
        .await
        .unwrap()
}

/// Credentials whose `expires_at` is `offset_secs` from now (negative = expired).
async fn seed_credentials(db: &SqlitePool, user: UserId, tid: i64, offset_secs: i64) {
    let expires = (time::OffsetDateTime::now_utc() + time::Duration::seconds(offset_secs))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    sqlx::query(
        "INSERT INTO user_tracker_credentials \
         (user_id, tracker_id, access_token, refresh_token, expires_at) \
         VALUES (?, ?, 'stored-access', 'stored-refresh', ?)",
    )
    .bind(user.0)
    .bind(tid)
    .bind(expires)
    .execute(db)
    .await
    .unwrap();
}

async fn needs_reauth(db: &SqlitePool, user: UserId, tid: i64) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT needs_reauth FROM user_tracker_credentials WHERE user_id = ? AND tracker_id = ?",
    )
    .bind(user.0)
    .bind(tid)
    .fetch_one(db)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_failed_refresh_marks_the_link_as_needing_reauth() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let tid = tracker_id(&svc.db).await;
    seed_credentials(&svc.db, user, tid, -60).await;

    let origin = TestOrigin::start().await;
    origin.set(
        "/token",
        Response::json(r#"{"error":"invalid_grant","error_description":"expired"}"#),
    );
    let tracker = MalTracker::new("client".into()).with_test_base(&origin.base());

    let res = get_access_token(&svc.db, tid, user, &tracker, None).await;
    assert!(res.is_err(), "a rejected refresh must surface an error");

    assert!(
        needs_reauth(&svc.db, user, tid).await,
        "a rejected refresh must mark the link as needing reauth — otherwise every \
         later sync retries the same doomed refresh forever"
    );
}

#[tokio::test]
async fn an_expired_token_is_refreshed_before_the_call() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let tid = tracker_id(&svc.db).await;
    seed_credentials(&svc.db, user, tid, -60).await;

    let origin = TestOrigin::start().await;
    origin.set(
        "/token",
        Response::json(
            r#"{"access_token":"refreshed-access","refresh_token":"r2","expires_in":3600}"#,
        ),
    );
    let tracker = MalTracker::new("client".into()).with_test_base(&origin.base());

    let token = get_access_token(&svc.db, tid, user, &tracker, None)
        .await
        .unwrap();

    assert_eq!(
        token, "refreshed-access",
        "an expired token must be refreshed before use, not handed back stale"
    );
    assert_eq!(
        origin.hits("/token"),
        1,
        "the refresh must actually reach the provider"
    );
}

#[tokio::test]
async fn a_valid_token_is_used_without_refreshing() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let tid = tracker_id(&svc.db).await;
    seed_credentials(&svc.db, user, tid, 3600).await;

    let origin = TestOrigin::start().await;
    origin.set("/token", Response::json(r#"{"access_token":"nope"}"#));
    let tracker = MalTracker::new("client".into()).with_test_base(&origin.base());

    let token = get_access_token(&svc.db, tid, user, &tracker, None)
        .await
        .unwrap();

    assert_eq!(token, "stored-access", "a live token is used as-is");
    assert_eq!(origin.hits("/token"), 0, "no refresh for a valid token");
}

#[tokio::test]
async fn a_revoked_token_is_reported_as_an_auth_failure() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/123", Response::status(401));
    let tracker = MalTracker::new("client".into()).with_test_base(&origin.base());

    let err = tracker
        .get_status("revoked-token", "123")
        .await
        .expect_err("a 401 must be an error");

    assert!(
        matches!(err, kani_app::error::ServiceError::TrackerAuthExpired(_)),
        "a 401 must be a distinguishable auth failure, not a generic parse error — \
         got {err:?}"
    );
}

#[tokio::test]
async fn a_revoked_token_is_recovered_from_reactively() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let tid = tracker_id(&svc.db).await;
    seed_credentials(&svc.db, user, tid, 3600).await;

    let origin = TestOrigin::start().await;
    origin.set("/manga/123", Response::status(401));
    {
        let mut registry = svc.tracker_registry.write().await;
        registry.trackers.insert(
            tid,
            Box::new(MalTracker::new("client".into()).with_test_base(&origin.base())),
        );
    }

    let source = common::insert_source(&svc.db, "src").await;
    let manga = common::insert_manga(&svc.db, source, "m1", "Tracked").await;
    svc.set_manga_tracking_enabled(user, manga, true)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tracker_manga_mappings (user_id, tracker_id, manga_id, tracker_manga_id) \
         VALUES (?, ?, ?, '123')",
    )
    .bind(user.0)
    .bind(tid)
    .bind(manga.0)
    .execute(&svc.db)
    .await
    .unwrap();

    let _ = svc.sync_manga_trackers(user, manga).await;

    assert!(
        needs_reauth(&svc.db, user, tid).await,
        "a 401 during sync must flag the link for re-authentication"
    );
}

#[tokio::test]
async fn a_partial_sync_failure_does_not_abort_the_remaining_entries() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let tid = tracker_id(&svc.db).await;
    seed_credentials(&svc.db, user, tid, 3600).await;

    let origin = TestOrigin::start().await;
    origin.set("/manga/bad", Response::status(500));
    origin.set(
        "/manga/good",
        Response::json(r#"{"my_list_status":{"status":"reading","num_chapters_read":3}}"#),
    );
    {
        let mut registry = svc.tracker_registry.write().await;
        registry.trackers.insert(
            tid,
            Box::new(MalTracker::new("client".into()).with_test_base(&origin.base())),
        );
    }

    let source = common::insert_source(&svc.db, "src").await;
    for (local, remote) in [("m-bad", "bad"), ("m-good", "good")] {
        let manga = common::insert_manga(&svc.db, source, local, local).await;
        svc.set_manga_tracking_enabled(user, manga, true)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tracker_manga_mappings (user_id, tracker_id, manga_id, tracker_manga_id) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(user.0)
        .bind(tid)
        .bind(manga.0)
        .bind(remote)
        .execute(&svc.db)
        .await
        .unwrap();
    }

    svc.sync_all_trackers(user).await.unwrap();

    assert_eq!(
        origin.hits("/manga/bad"),
        1,
        "the failing entry was attempted"
    );
    assert_eq!(
        origin.hits("/manga/good"),
        1,
        "and the failure did not abort the remaining entry"
    );
}

#[tokio::test]
async fn a_tracker_rate_limit_is_respected() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let tid = tracker_id(&svc.db).await;
    seed_credentials(&svc.db, user, tid, 3600).await;

    let origin = TestOrigin::start().await;
    origin.set(
        "/manga/limited",
        Response::status(429).header("Retry-After", "1"),
    );
    origin.set(
        "/manga/ok",
        Response::json(r#"{"my_list_status":{"status":"reading","num_chapters_read":1}}"#),
    );
    {
        let mut registry = svc.tracker_registry.write().await;
        registry.trackers.insert(
            tid,
            Box::new(MalTracker::new("client".into()).with_test_base(&origin.base())),
        );
    }

    let source = common::insert_source(&svc.db, "src").await;
    for (local, remote) in [("m-a", "limited"), ("m-b", "ok")] {
        let manga = common::insert_manga(&svc.db, source, local, local).await;
        svc.set_manga_tracking_enabled(user, manga, true)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tracker_manga_mappings (user_id, tracker_id, manga_id, tracker_manga_id) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(user.0)
        .bind(tid)
        .bind(manga.0)
        .bind(remote)
        .execute(&svc.db)
        .await
        .unwrap();
    }

    let cancel = tokio_util::sync::CancellationToken::new();
    let started = std::time::Instant::now();
    let outcome = svc
        .sync_stale_trackers(0, 10, std::time::Duration::ZERO, &cancel)
        .await
        .unwrap();
    let elapsed = started.elapsed();

    assert_eq!(outcome.rate_limited, 1, "the 429 was recognised as such");
    assert!(
        elapsed >= std::time::Duration::from_secs(1),
        "the Retry-After must be waited out before the next call to that \
         account, but the run finished in {elapsed:?}"
    );
}

#[tokio::test]
async fn tracker_tokens_are_never_written_to_the_support_bundle() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let tid = tracker_id(&svc.db).await;

    const SECRET_ACCESS: &str = "ACCESS-TOKEN-CANARY-a1b2c3";
    const SECRET_REFRESH: &str = "REFRESH-TOKEN-CANARY-d4e5f6";
    sqlx::query(
        "INSERT INTO user_tracker_credentials \
         (user_id, tracker_id, access_token, refresh_token) VALUES (?, ?, ?, ?)",
    )
    .bind(user.0)
    .bind(tid)
    .bind(SECRET_ACCESS)
    .bind(SECRET_REFRESH)
    .execute(&svc.db)
    .await
    .unwrap();

    let (zip_bytes, _name) = svc.generate_support_bundle(Vec::new()).await.unwrap();

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    for i in 0..archive.len() {
        use std::io::Read;
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().to_string();
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).unwrap();
        let text = String::from_utf8_lossy(&contents);
        assert!(
            !text.contains(SECRET_ACCESS) && !text.contains(SECRET_REFRESH),
            "a tracker token leaked into support bundle entry {name}"
        );
    }
}

#[tokio::test]
async fn storing_fresh_credentials_clears_the_reauth_flag() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let tid = tracker_id(&svc.db).await;
    seed_credentials(&svc.db, user, tid, -60).await;
    sqlx::query("UPDATE user_tracker_credentials SET needs_reauth = 1 WHERE user_id = ?")
        .bind(user.0)
        .execute(&svc.db)
        .await
        .unwrap();

    kani_app::service::trackers::store_credentials(
        &svc.db,
        user,
        tid,
        &kani_app::service::trackers::TokenResponse {
            access_token: "fresh".into(),
            refresh_token: Some("fresh-refresh".into()),
            expires_at: None,
        },
        None,
    )
    .await
    .unwrap();

    assert!(
        !needs_reauth(&svc.db, user, tid).await,
        "re-linking must clear the needs-reauth flag"
    );
}

/// A rejected link is only useful to the user if something tells them. The
/// degradation is reconciled from the table on each sync pass, so it also has to
/// clear itself once the link is restored.
#[tokio::test]
async fn rejected_credentials_raise_and_then_clear_a_degradation() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "reader").await;
    let tid = tracker_id(&svc.db).await;
    seed_credentials(&svc.db, user, tid, 3600).await;

    let cancel = tokio_util::sync::CancellationToken::new();
    svc.sync_stale_trackers(24, 10, std::time::Duration::ZERO, &cancel)
        .await
        .unwrap();
    assert!(
        !svc.degradations
            .list()
            .iter()
            .any(|d| d.id == kani_app::service::degradations::ids::TRACKER_CREDENTIALS),
        "a healthy link must not be reported as degraded"
    );

    sqlx::query("UPDATE user_tracker_credentials SET needs_reauth = TRUE WHERE user_id = ?")
        .bind(user.0)
        .execute(&svc.db)
        .await
        .unwrap();
    svc.sync_stale_trackers(24, 10, std::time::Duration::ZERO, &cancel)
        .await
        .unwrap();

    let raised = svc.degradations.list();
    let entry = raised
        .iter()
        .find(|d| d.id == kani_app::service::degradations::ids::TRACKER_CREDENTIALS)
        .expect("a rejected link must be reported");
    assert!(
        entry.remedy.to_lowercase().contains("re-link"),
        "the remedy must tell the user what to do, got: {}",
        entry.remedy
    );

    sqlx::query("UPDATE user_tracker_credentials SET needs_reauth = FALSE WHERE user_id = ?")
        .bind(user.0)
        .execute(&svc.db)
        .await
        .unwrap();
    svc.sync_stale_trackers(24, 10, std::time::Duration::ZERO, &cancel)
        .await
        .unwrap();

    assert!(
        !svc.degradations
            .list()
            .iter()
            .any(|d| d.id == kani_app::service::degradations::ids::TRACKER_CREDENTIALS),
        "re-linking must clear the entry, or it sticks until restart"
    );
}
