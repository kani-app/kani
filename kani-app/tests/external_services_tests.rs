#![allow(clippy::unwrap_used)]

//! External advisory services (HIBP breach check and GitHub update
//! check) driven against a TestOrigin via the base-URL seams. Both are
//! advisory: a failure must degrade silently, never block the caller.

use kani_app::service::password_policy::{
    PasswordPolicyError, check_password_with_hibp_base, sha1_hex_upper,
};
use kani_app::service::update_check::check_for_update_at;
use kani_core::http::SmartClient;
use kani_shared_test::origin::{Response, TestOrigin};

const STRONG_PW: &str = "correcthorsebatterystaple9!";

#[tokio::test]
async fn a_breached_password_is_rejected() {
    let hash = sha1_hex_upper(STRONG_PW);
    let (prefix, suffix) = hash.split_at(5);

    let origin = TestOrigin::start().await;
    origin.set(
        &format!("/range/{prefix}"),
        Response::ok(format!(
            "00000000000000000000000000000000000:1\r\n{suffix}:42\r\n"
        )),
    );

    let client = SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true);
    let res = check_password_with_hibp_base(STRONG_PW, "alice", &client, &origin.base()).await;
    assert!(
        matches!(res, Err(PasswordPolicyError::Pwned(42))),
        "a pwned password must be rejected with its breach count, got {res:?}"
    );
}

#[tokio::test]
async fn a_breach_check_failure_does_not_block_registration() {
    let hash = sha1_hex_upper(STRONG_PW);
    let (prefix, _) = hash.split_at(5);

    let origin = TestOrigin::start().await;
    origin.set(&format!("/range/{prefix}"), Response::status(500));

    let client = SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true);
    let res = check_password_with_hibp_base(STRONG_PW, "alice", &client, &origin.base()).await;
    assert!(
        res.is_ok(),
        "a HIBP failure must not block a strong password, got {res:?}"
    );
}

#[tokio::test]
async fn a_hostile_breach_response_is_bounded() {
    let hash = sha1_hex_upper(STRONG_PW);
    let (prefix, _) = hash.split_at(5);

    let origin = TestOrigin::start().await;
    origin.set(
        &format!("/range/{prefix}"),
        Response::ok(vec![b'A'; 4 * 1024 * 1024]),
    );

    let client = SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true);
    let res = check_password_with_hibp_base(STRONG_PW, "alice", &client, &origin.base()).await;

    assert!(
        res.is_ok(),
        "an oversized breach response must be abandoned, leaving the advisory \
         check inconclusive rather than blocking registration: {res:?}"
    );
    assert_eq!(
        res.unwrap().pwned_count,
        None,
        "the abandoned response yields no breach count at all"
    );
}

#[tokio::test]
async fn an_update_check_failure_is_silent_and_harmless() {
    let origin = TestOrigin::start().await;
    let client = SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true);

    origin.set("/releases", Response::status(500));
    assert!(
        check_for_update_at(&client, "0.9.0", &origin.url("/releases"))
            .await
            .is_none(),
        "a 500 from the releases endpoint yields no update info"
    );

    origin.set(
        "/releases",
        Response::html("<html>definitely not json</html>"),
    );
    assert!(
        check_for_update_at(&client, "0.9.0", &origin.url("/releases"))
            .await
            .is_none(),
        "a malformed body yields no update info"
    );
}

#[tokio::test]
async fn a_newer_release_tag_is_detected() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/releases",
        Response::json(r#"{"tag_name":"v1.2.3","html_url":"https://example/releases/v1.2.3"}"#),
    );
    let client = SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true);

    let info = check_for_update_at(&client, "0.9.0", &origin.url("/releases"))
        .await
        .expect("a newer tag must be reported");
    assert_eq!(info.latest, "1.2.3");
}
