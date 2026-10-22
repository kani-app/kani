#![allow(clippy::unwrap_used)]

//! Login lockout counts per client address, so whoever decides that address decides who gets
//! locked out. These drive the real router: a direct caller must not be able to name itself with
//! `X-Forwarded-For`, and a caller behind a configured proxy must still be told apart from its
//! neighbours.

mod common;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use common::{create_admin, test_state};
use std::net::SocketAddr;
use tower::ServiceExt;

/// A login attempt from `peer`, optionally claiming `forwarded` in `X-Forwarded-For`.
fn login_from(peer: &str, forwarded: Option<&str>, username: &str) -> Request<Body> {
    let body = serde_json::json!({ "username": username, "password": "definitely-wrong" });
    let mut builder = Request::builder()
        .method("POST")
        .uri("/rest/auth/login")
        .header("content-type", "application/json");
    if let Some(value) = forwarded {
        builder = builder.header("x-forwarded-for", value);
    }
    let mut req = builder.body(Body::from(body.to_string())).unwrap();
    let addr: SocketAddr = peer.parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    req
}

/// `max_ip_attempts` defaults to 20, so 21 failures from one address must trip the IP lockout.
const ATTEMPTS: usize = 21;

#[tokio::test]
async fn a_direct_caller_cannot_dodge_the_ip_lockout_by_forging_x_forwarded_for() {
    let state = test_state().await;
    create_admin(&state).await;
    assert!(
        state.trusted_proxies.is_empty(),
        "the default deployment trusts no proxy"
    );
    let app = common::build_test_app(state).await;

    let mut last = StatusCode::OK;
    for i in 0..ATTEMPTS {
        // A different forged client every time, and a different username so the identity
        // lockout cannot be what fires.
        let forged = format!("203.0.113.{}", i + 1);
        let res = app
            .clone()
            .oneshot(login_from(
                "198.51.100.7:44444",
                Some(&forged),
                &format!("ghost{i}"),
            ))
            .await
            .unwrap();
        last = res.status();
    }

    assert_eq!(
        last,
        StatusCode::TOO_MANY_REQUESTS,
        "rotating X-Forwarded-For must not reset the counter: the peer address is the client \
         when no proxy is trusted"
    );
}

#[tokio::test]
async fn a_forged_header_cannot_lock_out_a_bystander() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = common::build_test_app(state).await;

    // An attacker blames a victim address for every failure.
    for i in 0..ATTEMPTS {
        let _ = app
            .clone()
            .oneshot(login_from(
                "198.51.100.7:44444",
                Some("203.0.113.42"),
                &format!("ghost{i}"),
            ))
            .await
            .unwrap();
    }

    // The victim, arriving from the address that was blamed, can still log in.
    let body = serde_json::json!({ "username": username, "password": password });
    let mut req = Request::builder()
        .method("POST")
        .uri("/rest/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(
        "203.0.113.42:33333".parse::<SocketAddr>().unwrap(),
    ));

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "an attacker naming a victim in X-Forwarded-For must not lock the victim out"
    );
}

#[tokio::test]
async fn behind_a_trusted_proxy_each_forwarded_client_gets_its_own_budget() {
    let mut state = test_state().await;
    let (trusted, rejected) = kani_web::client_ip::TrustedProxies::parse("198.51.100.0/24");
    assert!(rejected.is_empty());
    state.trusted_proxies = std::sync::Arc::new(trusted);
    let (username, password) = create_admin(&state).await;
    let app = common::build_test_app(state).await;

    // One forwarded client burns through the IP budget.
    for i in 0..ATTEMPTS {
        let _ = app
            .clone()
            .oneshot(login_from(
                "198.51.100.7:44444",
                Some("203.0.113.1"),
                &format!("ghost{i}"),
            ))
            .await
            .unwrap();
    }

    // A different forwarded client through the same proxy is unaffected.
    let body = serde_json::json!({ "username": username, "password": password });
    let mut req = Request::builder()
        .method("POST")
        .uri("/rest/auth/login")
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.2")
        .body(Body::from(body.to_string()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(
        "198.51.100.7:44444".parse::<SocketAddr>().unwrap(),
    ));

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "keying on the proxy's own address would put every client in one bucket"
    );
}
