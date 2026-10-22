#![allow(clippy::unwrap_used)]

//! Webhook delivery. `create_webhook` refuses loopback URLs (SSRF
//! guard, covered by webhook_ssrf_tests), so delivery is exercised by inserting
//! the row directly and opting the egress guard into private hosts — the same
//! pattern jobs_tests uses for the delivery job.

mod common;
use common::test_service;

use hmac::{Hmac, Mac};
use kani_shared_test::origin::{Response, TestOrigin};
use sha2::Sha256;

#[tokio::test]
async fn a_webhook_delivery_records_its_status() {
    let svc = test_service().await;
    svc.webhook_service.allow_private_egress_for_test();
    let origin = TestOrigin::start().await;

    let webhook_id: i64 =
        sqlx::query_scalar("INSERT INTO webhooks (url, enabled) VALUES (?, 1) RETURNING id")
            .bind(origin.url("/hook"))
            .fetch_one(&svc.db)
            .await
            .unwrap();

    origin.set("/hook", Response::json("{}"));
    let (status, err) = svc
        .webhook_service
        .send_signed(&origin.url("/hook"), None, "{}")
        .await;
    svc.webhook_service
        .record_delivery(webhook_id, "chapter.new", "{}", status, err)
        .await;

    origin.set("/hook", Response::status(500));
    let (status2, err2) = svc
        .webhook_service
        .send_signed(&origin.url("/hook"), None, "{}")
        .await;
    svc.webhook_service
        .record_delivery(webhook_id, "chapter.new", "{}", status2, err2)
        .await;

    let statuses: Vec<Option<i64>> = sqlx::query_scalar(
        "SELECT http_status FROM webhook_deliveries WHERE webhook_id = ? ORDER BY id",
    )
    .bind(webhook_id)
    .fetch_all(&svc.db)
    .await
    .unwrap();
    assert_eq!(
        statuses,
        vec![Some(200), Some(500)],
        "each delivery records the actual upstream status"
    );
}

#[tokio::test]
async fn the_hmac_signature_matches_the_delivered_body() {
    let svc = test_service().await;
    svc.webhook_service.allow_private_egress_for_test();
    let origin = TestOrigin::start().await;
    origin.set("/hook", Response::json("{}"));

    let body = r#"{"event":"chapter.new","id":42}"#;
    let secret = "s3cr3t-key";
    let (status, err) = svc
        .webhook_service
        .send_signed(&origin.url("/hook"), Some(secret), body)
        .await;
    assert_eq!(status, Some(200));
    assert!(err.is_none());

    let seen = origin
        .last_request("/hook")
        .expect("the webhook was delivered");
    let sig = seen
        .header("x-kani-signature")
        .expect("the signature header is present");

    let mut mac = <Hmac<Sha256>>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body.as_bytes());
    let expected = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    assert_eq!(
        sig, expected,
        "X-Kani-Signature must be HMAC-SHA256 of the exact delivered body"
    );
}

#[tokio::test]
async fn a_webhook_is_not_retried_into_a_duplicate_delivery() {
    let svc = test_service().await;
    svc.webhook_service.allow_private_egress_for_test();
    let origin = TestOrigin::start().await;
    origin.set("/hook", Response::json("{}"));

    let webhook_id: i64 =
        sqlx::query_scalar("INSERT INTO webhooks (url, enabled) VALUES (?, 1) RETURNING id")
            .bind(origin.url("/hook"))
            .fetch_one(&svc.db)
            .await
            .unwrap();

    let job_id = svc
        .job_manager
        .submit(kani_app::jobs::webhook_delivery::WebhookDeliveryJob::new(
            webhook_id,
            "chapter.new".to_string(),
            "{}".to_string(),
        ))
        .await
        .unwrap();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let status: Option<String> = sqlx::query_scalar("SELECT status FROM jobs WHERE id = ?")
            .bind(job_id.to_string())
            .fetch_optional(&svc.db)
            .await
            .unwrap();
        if matches!(status.as_deref(), Some("completed") | Some("failed")) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "webhook delivery job never finished"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    assert_eq!(
        origin.hits("/hook"),
        1,
        "a successful delivery is posted exactly once"
    );
    let deliveries: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM webhook_deliveries WHERE webhook_id = ?")
            .bind(webhook_id)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert_eq!(deliveries, 1, "and recorded exactly once");
}

#[tokio::test]
async fn an_oversized_webhook_response_is_not_buffered() {
    let svc = test_service().await;
    svc.webhook_service.allow_private_egress_for_test();
    let origin = TestOrigin::start().await;
    origin.set("/hook", Response::ok(vec![b'X'; 8 * 1024 * 1024]));

    let started = std::time::Instant::now();
    let (status, err) = svc
        .webhook_service
        .send_signed(&origin.url("/hook"), None, "{}")
        .await;
    let elapsed = started.elapsed();

    assert_eq!(status, Some(200), "the status is still read: {err:?}");
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "an 8 MB reply must not be buffered into the delivery path, took {elapsed:?}"
    );
}

#[tokio::test]
async fn an_unsigned_webhook_carries_no_signature_header() {
    let svc = test_service().await;
    svc.webhook_service.allow_private_egress_for_test();
    let origin = TestOrigin::start().await;
    origin.set("/hook", Response::json("{}"));

    svc.webhook_service
        .send_signed(&origin.url("/hook"), None, "{}")
        .await;

    let seen = origin.last_request("/hook").expect("delivered");
    assert!(
        seen.header("x-kani-signature").is_none(),
        "a webhook without a secret must not carry a signature header"
    );
}
