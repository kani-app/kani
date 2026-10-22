#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use axum::{body::Body, http::Request};
use common::{
    basic_auth, build_test_app_with_opds, create_admin, insert_chapter, insert_manga,
    insert_source, login, test_state,
};
use http_body_util::BodyExt as _;
use kani_app::ids::{ChapterId, UserId};
use kani_web::state::AppState;
use std::io::Write;
use tower::ServiceExt;

fn make_png(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img = image::ImageBuffer::from_pixel(w, h, image::Rgb([r, g, b]));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

fn write_cbz(path: &std::path::Path, pages: &[Vec<u8>]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    for (i, data) in pages.iter().enumerate() {
        w.start_file(format!("{:04}.png", i + 1), opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
}

async fn seed_downloaded_chapter(state: &AppState) -> ChapterId {
    let src = insert_source(&state.db, "src").await;
    let manga = insert_manga(&state.db, src, "m1", "Test Manga").await;
    let ch = insert_chapter(&state.db, manga, "c1", 1.0).await;
    sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
        .bind(ch)
        .execute(&state.db)
        .await
        .unwrap();

    let library_path = state.service.settings.read().await.library_path.clone();
    let manga_dir = library_path.join(format!(
        "{} - {}",
        kani_core::utilities::sanitize_filename("Test Manga"),
        manga.0
    ));
    std::fs::create_dir_all(&manga_dir).unwrap();

    let pages = vec![
        make_png(4, 6, 10, 20, 30),
        make_png(4, 8, 40, 50, 60),
        make_png(400, 300, 70, 80, 90),
    ];
    let info = state.service.chapter_cbz_path(ch).await.unwrap();
    write_cbz(&info.path, &pages);
    ch
}

async fn admin_user_id(state: &AppState) -> UserId {
    let id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'admin'")
        .fetch_one(&state.db)
        .await
        .unwrap();
    UserId(id)
}

fn bearer_get(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn chapter_feed_auth_matrix() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let ch = seed_downloaded_chapter(&state).await;
    let uid = admin_user_id(&state).await;
    let token = state
        .service
        .create_token(
            uid,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap()
        .raw_token;
    let app = build_test_app_with_opds(state).await;
    let uri = format!("/opds/chapters/{}", ch.0);

    let cookie = login(&app, u, p).await;
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("Cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app.clone().oneshot(bearer_get(&uri, &token)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("xmlns:pse"));
    assert!(text.contains(r#"pse:count="3""#));
    assert!(text.contains("page?page={pageNumber}"));

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("Authorization", basic_auth("admin", &token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn page_endpoint_validation() {
    let state = test_state().await;
    create_admin(&state).await;
    let ch = seed_downloaded_chapter(&state).await;
    let uid = admin_user_id(&state).await;
    let token = state
        .service
        .create_token(
            uid,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap()
        .raw_token;
    let app = build_test_app_with_opds(state).await;

    let res = app
        .clone()
        .oneshot(bearer_get(
            &format!("/opds/chapters/{}/page?page=1", ch.0),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .clone()
        .oneshot(bearer_get(
            &format!("/opds/chapters/{}/page?page=0", ch.0),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = app
        .clone()
        .oneshot(bearer_get(&format!("/opds/chapters/{}/page", ch.0), &token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = app
        .clone()
        .oneshot(bearer_get(
            &format!("/opds/chapters/{}/page?page=0&format=bmp", ch.0),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = app
        .clone()
        .oneshot(bearer_get(
            &format!("/opds/chapters/{}/page?page=99", ch.0),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn file_endpoint_supports_range() {
    let state = test_state().await;
    create_admin(&state).await;
    let ch = seed_downloaded_chapter(&state).await;
    let uid = admin_user_id(&state).await;
    let token = state
        .service
        .create_token(
            uid,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap()
        .raw_token;
    let app = build_test_app_with_opds(state).await;
    let uri = format!("/opds/chapters/{}/file", ch.0);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("Authorization", format!("Bearer {token}"))
                .header("Range", "bytes=0-99")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    let cr = res
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    assert!(cr.starts_with("bytes 0-99/"), "content-range: {cr}");
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 100);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("Authorization", format!("Bearer {token}"))
                .header("Range", "bytes=999999999-1000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
}

#[tokio::test]
async fn file_endpoint_streams_full_download() {
    let state = test_state().await;
    create_admin(&state).await;
    let ch = seed_downloaded_chapter(&state).await;
    let uid = admin_user_id(&state).await;
    let token = state
        .service
        .create_token(
            uid,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap()
        .raw_token;
    let expected_len = {
        let info = state.service.chapter_cbz_path(ch).await.unwrap();
        std::fs::metadata(&info.path).unwrap().len()
    };
    let app = build_test_app_with_opds(state).await;

    let res = app
        .clone()
        .oneshot(bearer_get(&format!("/opds/chapters/{}/file", ch.0), &token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/vnd.comicbook+zip"),
    );
    let content_length = res
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    assert_eq!(content_length, Some(expected_len));
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        body.len() as u64,
        expected_len,
        "full file must stream in its entirety"
    );
    assert_eq!(&body[..2], b"PK", "streamed body should be a valid zip");
}

#[tokio::test]
async fn progress_push_updates_last_read() {
    let state = test_state().await;
    create_admin(&state).await;
    let ch = seed_downloaded_chapter(&state).await;
    let uid = admin_user_id(&state).await;
    let token = state
        .service
        .create_token(
            uid,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap()
        .raw_token;
    let service = state.service.clone();
    let app = build_test_app_with_opds(state).await;

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/opds/chapters/{}/progress", ch.0))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"page":2}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    service.flush_progress_buffer().await;

    let res = app
        .clone()
        .oneshot(bearer_get(&format!("/opds/chapters/{}", ch.0), &token))
        .await
        .unwrap();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains(r#"pse:lastRead="2""#), "feed: {text}");
}

#[tokio::test]
async fn a_reported_page_is_stored_as_a_zero_based_index() {
    let state = test_state().await;
    create_admin(&state).await;
    let ch = seed_downloaded_chapter(&state).await;
    let uid = admin_user_id(&state).await;
    let service = state.service.clone();
    let token = service
        .create_token(
            uid,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap()
        .raw_token;
    let app = build_test_app_with_opds(state).await;

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/opds/chapters/{}/progress", ch.0))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"page":2}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    service.flush_progress_buffer().await;

    let stored = service.get_chapter_progress(uid, ch).await.unwrap();
    assert_eq!(
        stored.map(|(p, _)| p),
        Some(1),
        "OPDS page 2 is the second page, stored as index 1"
    );
}

#[tokio::test]
async fn progress_post_with_bad_body_is_rejected() {
    let state = test_state().await;
    create_admin(&state).await;
    let ch = seed_downloaded_chapter(&state).await;
    let uid = admin_user_id(&state).await;
    let token = state
        .service
        .create_token(
            uid,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap()
        .raw_token;
    let app = build_test_app_with_opds(state).await;

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/opds/chapters/{}/progress", ch.0))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"page":"not-a-number"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(res.status().is_client_error());
}

#[tokio::test]
async fn token_semantics() {
    let state = test_state().await;
    create_admin(&state).await;
    let ch = seed_downloaded_chapter(&state).await;
    let uid = admin_user_id(&state).await;

    let revoked = state
        .service
        .create_token(
            uid,
            "revoked",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();
    state
        .service
        .revoke_api_token(uid, &revoked.token.id)
        .await
        .unwrap();

    let expired = state
        .service
        .create_token(
            uid,
            "expired",
            Some(1),
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();
    sqlx::query("UPDATE api_tokens SET expires_at = unixepoch() - 10 WHERE id = ?")
        .bind(&expired.token.id)
        .execute(&state.db)
        .await
        .unwrap();

    let readonly = state
        .service
        .create_token(
            uid,
            "readonly",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();
    sqlx::query("UPDATE api_tokens SET scopes = 'opds:read' WHERE id = ?")
        .bind(&readonly.token.id)
        .execute(&state.db)
        .await
        .unwrap();

    let good = state
        .service
        .create_token(
            uid,
            "basic",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();

    let app = build_test_app_with_opds(state).await;
    let feed_uri = format!("/opds/chapters/{}", ch.0);

    let res = app
        .clone()
        .oneshot(bearer_get(&feed_uri, &revoked.raw_token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = app
        .clone()
        .oneshot(bearer_get(&feed_uri, &expired.raw_token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = app
        .clone()
        .oneshot(bearer_get(&feed_uri, &readonly.raw_token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/opds/chapters/{}/progress", ch.0))
                .header("Authorization", format!("Bearer {}", readonly.raw_token))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"page":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&feed_uri)
                .header("Authorization", basic_auth("not-admin", &good.raw_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
