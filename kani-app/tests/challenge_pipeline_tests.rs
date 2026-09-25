#![allow(clippy::unwrap_used)]

//! Challenge and FlareSolverr fallback behavior in `kani_core::http::SmartClient`.
//! A `TestOrigin` plays the protected site and a second one plays the fake solver, returning the
//! FlareSolverr envelope. The 503/403 paths are used deliberately: neither status
//! is retryable, so these never hit the 5 s×2^n retry backoff.

use kani_core::http::{SmartClient, SolverCaptureError, Timings};
use kani_shared_test::origin::{Body, Response, TestOrigin};
use std::time::Duration;

/// A FlareSolverr `request.get` success envelope. `rendered` is echoed back as
/// `solution.response` (the HTML the solver "saw" after clearing the challenge).
fn solver_envelope(rendered: &str) -> String {
    serde_json::json!({
        "status": "ok",
        "solution": {
            "userAgent": "FlareUA/1.0",
            "response": rendered,
            "cookies": [{ "name": "cf_clearance", "value": "TOKEN123" }],
        },
    })
    .to_string()
}

/// A solver index advertising scripted capture. The capability probe reads this
/// before dispatching, so a mock that serves only /v1 reads as unreachable.
const CAPABLE_INDEX: &str =
    r#"{"msg":"ready","capabilities":["kani.capture/1","kani.capture/2","kani.egress-guard/1"]}"#;

#[tokio::test]
async fn a_challenge_page_triggers_the_solver_and_replays() {
    let site = TestOrigin::start().await;
    site.set(
        "/page",
        Response::html("<html><body>Just a moment...</body></html>"),
    );
    let solver = TestOrigin::start().await;
    solver.set(
        "/v1",
        Response::json(&solver_envelope("<html><body>REAL CONTENT</body></html>")),
    );

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true);
    let resp = client.get(&site.url("/page")).await.unwrap();
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("REAL CONTENT"),
        "the caller gets the solver-rendered page, got: {body}"
    );
    assert!(solver.hits("/v1") >= 1, "the solver was invoked");
}

#[tokio::test]
async fn the_solved_cookie_is_attached_to_the_replay() {
    let site = TestOrigin::start().await;
    site.script(
        "/page",
        vec![Response::status(503), Response::html("<html>ok</html>")],
    );
    let solver = TestOrigin::start().await;
    solver.set("/v1", Response::json(&solver_envelope("unused")));

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true);
    let resp = client.get(&site.url("/page")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let replay = site.last_request("/page").unwrap();
    let cookie = replay.header("cookie").unwrap_or("");
    assert!(
        cookie.contains("cf_clearance=TOKEN123"),
        "the replay carried the solved cookie, got: {cookie:?}"
    );
}

#[tokio::test]
async fn a_solver_error_status_surfaces_as_a_useful_error() {
    let site = TestOrigin::start().await;
    site.set("/page", Response::status(503));
    let solver = TestOrigin::start().await;
    solver.set(
        "/v1",
        Response::json(r#"{"status":"error","message":"challenge-boom"}"#),
    );

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true);
    let msg = match client.get(&site.url("/page")).await {
        Ok(_) => panic!("expected the solver error to surface, got a response"),
        Err(e) => e.to_string(),
    };

    assert!(
        msg.contains("challenge-boom") || msg.to_lowercase().contains("flaresolverr"),
        "the solver error is surfaced, got: {msg}"
    );
}

#[tokio::test]
async fn stored_credentials_are_re_solved_after_a_403() {
    let site = TestOrigin::start().await;
    site.script(
        "/page",
        vec![
            Response::status(403),
            Response::html("<html>ok1</html>"),
            Response::status(403),
            Response::html("<html>ok2</html>"),
        ],
    );
    let solver = TestOrigin::start().await;
    solver.set("/v1", Response::json(&solver_envelope("unused")));

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true);
    assert_eq!(
        client
            .get(&site.url("/page"))
            .await
            .unwrap()
            .status()
            .as_u16(),
        200
    );
    assert_eq!(
        client
            .get(&site.url("/page"))
            .await
            .unwrap()
            .status()
            .as_u16(),
        200
    );

    assert_eq!(
        solver.hits("/v1"),
        2,
        "the second 403 dropped the stored credential and forced a fresh solve"
    );
}

#[tokio::test]
async fn expired_credentials_are_dropped_before_reuse() {
    let site = TestOrigin::start().await;
    site.script(
        "/page",
        vec![
            Response::status(503),
            Response::html("<html>ok</html>"),
            Response::html("<html>ok</html>"),
        ],
    );
    let solver = TestOrigin::start().await;
    solver.set("/v1", Response::json(&solver_envelope("unused")));

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true)
        .with_timings(Timings {
            credential_ttl: Duration::from_millis(20),
            ..Timings::default()
        });

    client.get(&site.url("/page")).await.unwrap();
    assert!(
        site.last_request("/page")
            .unwrap()
            .header("cookie")
            .unwrap_or("")
            .contains("cf_clearance"),
        "sanity: the freshly-solved credential rode the replay"
    );

    tokio::time::sleep(Duration::from_millis(60)).await;
    client.get(&site.url("/page")).await.unwrap();

    let cookie = site
        .last_request("/page")
        .unwrap()
        .header("cookie")
        .unwrap_or("")
        .to_string();
    assert!(
        !cookie.contains("cf_clearance"),
        "the expired credential was dropped rather than reused, got: {cookie:?}"
    );
}

#[tokio::test]
async fn a_solver_that_is_unreachable_does_not_hang_the_request() {
    let site = TestOrigin::start().await;
    site.set("/page", Response::status(503));
    let solver = TestOrigin::start().await;
    solver.set("/v1", Response::status(200).body(Body::Stall));

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true)
        .with_timings(Timings {
            solver_timeout: Duration::from_millis(300),
            ..Timings::default()
        });

    let outcome = tokio::time::timeout(Duration::from_secs(10), client.get(&site.url("/page")))
        .await
        .expect("the request must return, not hang, when the solver stalls");

    assert!(
        outcome.is_err(),
        "an unreachable solver surfaces as an error"
    );
}

#[tokio::test]
async fn without_a_solver_a_challenge_is_passed_through() {
    let site = TestOrigin::start().await;
    site.set("/page", Response::status(503));

    let client = SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true);
    let resp = client.get(&site.url("/page")).await.unwrap();

    assert_eq!(
        resp.status().as_u16(),
        503,
        "the raw challenge status is returned"
    );
}

/// A `kani.capture` success envelope, as the Kani-compatible solver image
/// returns it after solving, injecting, reloading, and polling `passPayload`.
fn capture_envelope(payload: &str) -> String {
    serde_json::json!({
        "status": "ok",
        "message": "Challenge solved!",
        "solution": {
            "userAgent": "FlareUA/1.0",
            "cookies": [{ "name": "cf_clearance", "value": "TOKEN123" }],
            "payload": payload,
        },
    })
    .to_string()
}

#[tokio::test]
async fn a_capture_returns_the_payload_the_solver_browser_collected() {
    let solver = TestOrigin::start().await;
    solver.set("/", Response::json(CAPABLE_INDEX));
    solver.set(
        "/v1",
        Response::json(&capture_envelope(r#"{"items":[1,2]}"#)),
    );

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true);
    let payload = client
        .solver_capture(
            "https://site.test/browse",
            "passPayload('x')",
            30000,
            None,
            false,
        )
        .await
        .unwrap();

    assert_eq!(payload, r#"{"items":[1,2]}"#);
    assert!(solver.hits("/v1") >= 1, "the solver was invoked");
}

#[tokio::test]
async fn a_stock_solver_is_reported_as_unsupported_not_as_a_failure() {
    let solver = TestOrigin::start().await;
    solver.set(
        "/",
        Response::json(r#"{"msg":"FlareSolverr is ready!","version":"3.5.0"}"#),
    );
    solver.set("/v1", Response::json(r#"{"status":"ok","sessions":[]}"#));

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true);
    let error = client
        .solver_capture(
            "https://site.test/browse",
            "passPayload('x')",
            30000,
            None,
            false,
        )
        .await
        .expect_err("a stock solver cannot capture");

    assert!(
        matches!(error, SolverCaptureError::Unsupported),
        "an index without a capabilities array identifies a stock solver, got: {error:?}"
    );
    assert!(
        error.to_string().contains("kani.capture"),
        "the message names the missing command, got: {error}"
    );

    let hits_after_probe = solver.hits("/v1");
    let second = client
        .solver_capture(
            "https://site.test/second",
            "passPayload('x')",
            30000,
            None,
            false,
        )
        .await
        .expect_err("the cached capability result rejects another capture");
    assert!(matches!(second, SolverCaptureError::Unsupported));
    assert_eq!(
        solver.hits("/v1"),
        hits_after_probe,
        "the cached result must not cost another round trip"
    );
}

#[tokio::test]
async fn a_solver_that_rejects_the_key_is_reported_as_unauthorized() {
    let solver = TestOrigin::start().await;
    solver.set(
        "/",
        Response::json(r#"{"msg":"ready","capabilities":["kani.capture/2"]}"#),
    );
    solver.set("/v1", Response::status(401));

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true);
    let error = client
        .solver_capture(
            "https://site.test/browse",
            "passPayload('x')",
            30000,
            None,
            false,
        )
        .await
        .expect_err("a rejected key cannot capture");

    assert!(
        matches!(error, SolverCaptureError::Unauthorized),
        "a 401 is a key problem, not an incompatible image, got: {error:?}"
    );
    assert!(
        error.to_string().contains("KANI_SOLVER_SECRET"),
        "the message names the setting to fix, got: {error}"
    );
}

#[tokio::test]
async fn a_capture_error_from_a_compatible_solver_surfaces_verbatim() {
    let solver = TestOrigin::start().await;
    solver.set("/", Response::json(CAPABLE_INDEX));
    solver.set(
        "/v1",
        Response::json(
            r#"{"status":"error","message":"Error capturing the payload. passPayload was not called within 30000 ms."}"#,
        ),
    );

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true);
    let error = client
        .solver_capture(
            "https://site.test/browse",
            "passPayload('x')",
            30000,
            None,
            false,
        )
        .await
        .expect_err("the capture failed");

    assert!(
        !matches!(error, SolverCaptureError::Unsupported),
        "a real capture failure is not mistaken for an incompatible solver"
    );
    assert!(
        error.to_string().contains("passPayload was not called"),
        "the solver's diagnosis reaches the caller, got: {error}"
    );
}

#[tokio::test]
async fn an_ok_envelope_without_a_payload_is_an_error_not_an_empty_capture() {
    let solver = TestOrigin::start().await;
    solver.set("/", Response::json(CAPABLE_INDEX));
    solver.set(
        "/v1",
        Response::json(r#"{"status":"ok","solution":{"userAgent":"FlareUA/1.0"}}"#),
    );

    let client = SmartClient::new(Some(solver.url("/v1")))
        .unwrap()
        .with_allow_loopback_egress(true);
    let error = client
        .solver_capture(
            "https://site.test/browse",
            "passPayload('x')",
            30000,
            None,
            false,
        )
        .await
        .expect_err("a missing payload must not read as a successful empty capture");

    assert!(error.to_string().contains("no payload"), "got: {error}");
}
