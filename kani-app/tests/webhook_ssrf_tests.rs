#![allow(clippy::unwrap_used)]

mod common;
use common::test_service;

use kani_app::service::webhooks::CreateWebhookBody;

fn body(url: &str) -> CreateWebhookBody {
    CreateWebhookBody {
        url: url.to_owned(),
        secret: None,
        events: None,
    }
}

#[tokio::test]
async fn create_webhook_rejects_cloud_metadata_endpoint() {
    let svc = test_service().await;
    let err = svc
        .webhook_service
        .create_webhook(body("http://169.254.169.254/latest/meta-data/"))
        .await
        .expect_err("a webhook at the cloud metadata endpoint must be refused");
    assert!(
        matches!(err, kani_app::error::ServiceError::Validation(_)),
        "expected a validation error, got {err:?}"
    );
}

#[tokio::test]
async fn create_webhook_rejects_loopback_and_private_literals() {
    let svc = test_service().await;
    for url in [
        "http://127.0.0.1:9000/hook",
        "https://10.0.0.5/hook",
        "https://192.168.1.10/hook",
        "http://[::1]:8080/hook",
    ] {
        let err = svc
            .webhook_service
            .create_webhook(body(url))
            .await
            .unwrap_err();
        assert!(
            matches!(err, kani_app::error::ServiceError::Validation(_)),
            "{url} must be refused, got {err:?}"
        );
    }
}

#[tokio::test]
async fn create_webhook_accepts_a_public_url() {
    let svc = test_service().await;
    let created = svc
        .webhook_service
        .create_webhook(body("https://example.com/hook"))
        .await
        .expect("a public webhook URL must be accepted");
    assert_eq!(created.url, "https://example.com/hook");
}

#[tokio::test]
async fn send_signed_refuses_a_forbidden_host_without_dialling() {
    let svc = test_service().await;
    let (status, error) = svc
        .webhook_service
        .send_signed("http://127.0.0.1:9/hook", None, "{}")
        .await;
    assert!(
        status.is_none(),
        "no HTTP status: the request must not be made"
    );
    let error = error.expect("a refusal reason must be returned");
    assert!(
        error.contains("Refused"),
        "expected a refusal, got {error:?}"
    );
}
