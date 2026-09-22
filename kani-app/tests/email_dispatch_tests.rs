#![allow(clippy::unwrap_used)]

//! Does a password reset actually put an email on the wire?
//!
//! KANI-1: `EmailTransport` had one implementation and no test reached the
//! boundary, so nothing asserted that any of the flows that are supposed to
//! send mail ever call it. These drive the public entry points against a
//! capturing transport.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{insert_user, test_service};
use kani_app::service::email::{CapturingEmailTransport, EmailService};

const WITHIN: Duration = Duration::from_secs(5);

/// Swaps the service's mailer for one that records instead of sending, and
/// turns reset mail on — `new_for_test` ships it disabled, so without this the
/// flow returns `Ok(())` having sent nothing and the test passes vacuously.
async fn capture(svc: &kani_app::service::AppService) -> Arc<CapturingEmailTransport> {
    svc.settings.write().await.password_reset_enabled = true;
    let transport = Arc::new(CapturingEmailTransport::new());
    let mailer =
        EmailService::with_capture("Kani <kani@example.com>".to_owned(), Arc::clone(&transport));
    *svc.email_service.write().await = Some(mailer);
    transport
}

#[tokio::test]
async fn requesting_a_password_reset_sends_an_email_to_that_user() {
    let svc = test_service().await;
    insert_user(&svc.db, "alice").await;
    let transport = capture(&svc).await;

    svc.request_password_reset("alice@test.local")
        .await
        .unwrap();

    let sent = transport.wait_for(1, WITHIN).await;
    assert_eq!(
        sent.len(),
        1,
        "a reset request must dispatch exactly one email"
    );
    assert_eq!(sent[0].to, "alice@test.local");
    assert_eq!(sent[0].from, "Kani <kani@example.com>");
    assert!(
        sent[0].html_body.contains("reset-password?token="),
        "the body must carry the reset link the user has to follow, got: {}",
        sent[0].html_body
    );
}

#[tokio::test]
async fn a_reset_for_an_unknown_address_sends_nothing() {
    let svc = test_service().await;
    insert_user(&svc.db, "alice").await;
    let transport = capture(&svc).await;

    // The handler returns Ok either way so it cannot be used to enumerate
    // accounts; the observable difference is whether mail goes out.
    svc.request_password_reset("nobody@test.local")
        .await
        .unwrap();

    assert!(
        transport
            .wait_for(1, Duration::from_millis(750))
            .await
            .is_empty(),
        "an unregistered address must not receive mail"
    );
}
