#![allow(clippy::unwrap_used)]

//! SmartClient circuit-breaker and transport-error retry behavior, driven by
//! real origin responses. `Timings` shortens the retry backoff/jitter and the circuit
//! cooldown so these exercise the production code paths in milliseconds.

use kani_core::http::{SmartClient, Timings};
use kani_shared_test::origin::{Body, Response, TestOrigin};
use std::time::Duration;

/// Near-instant retries so the real retry/circuit loop runs without minute-long
/// waits. `cooldown` remains separate so recovery timing can vary by scenario.
fn fast_timings(cooldown: Duration) -> Timings {
    Timings {
        retry_base_delay: Duration::from_millis(1),
        retry_jitter: Duration::ZERO,
        circuit_cooldown: cooldown,
        ..Timings::default()
    }
}

fn fast_client() -> SmartClient {
    SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true)
        .with_timings(fast_timings(Duration::from_secs(30)))
}

#[tokio::test]
async fn the_circuit_opens_after_repeated_real_failures() {
    let site = TestOrigin::start().await;
    site.set("/x", Response::status(502));
    let client = fast_client();

    for _ in 0..5 {
        let _ = client.get(&site.url("/x")).await;
    }

    let hits_before = site.hits("/x");
    let res = client.get(&site.url("/x")).await;

    assert!(res.is_err(), "an open circuit rejects the call");
    assert_eq!(
        site.hits("/x"),
        hits_before,
        "an open circuit never reaches the socket"
    );
}

#[tokio::test]
async fn the_circuit_recovers_after_the_cooldown() {
    let site = TestOrigin::start().await;
    site.set("/x", Response::status(502));
    let client = SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true)
        .with_timings(fast_timings(Duration::from_millis(50)));

    for _ in 0..5 {
        let _ = client.get(&site.url("/x")).await;
    }
    assert!(
        client.get(&site.url("/x")).await.is_err(),
        "the circuit is open immediately after the threshold"
    );

    site.set("/x", Response::html("<html>ok</html>"));
    tokio::time::sleep(Duration::from_millis(120)).await;

    let status = client
        .get(&site.url("/x"))
        .await
        .expect("after the cooldown the circuit half-opens and the call goes through")
        .status();
    assert_eq!(status.as_u16(), 200);
}

#[tokio::test]
async fn a_slow_body_hits_the_request_timeout_not_a_hang() {
    let site = TestOrigin::start().await;
    site.set("/x", Response::status(200).body(Body::Stall));
    let client = SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true)
        .with_timings(Timings {
            retry_base_delay: Duration::from_millis(1),
            retry_jitter: Duration::ZERO,
            request_timeout: Duration::from_millis(80),
            ..Timings::default()
        });

    let start = std::time::Instant::now();
    let res = client.get(&site.url("/x")).await;
    let elapsed = start.elapsed();

    assert!(
        res.is_err(),
        "a stalled body must surface a timeout error, not a value"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the call must return promptly via the request timeout, not hang (took {elapsed:?})"
    );
}

#[tokio::test]
async fn a_connection_reset_mid_request_is_retried() {
    let site = TestOrigin::start().await;
    site.script(
        "/x",
        vec![
            Response::status(200).body(Body::Reset),
            Response::status(200).body(Body::Reset),
            Response::html("<html>ok</html>"),
        ],
    );
    let client = fast_client();

    let status = client
        .get(&site.url("/x"))
        .await
        .expect("two resets then a success must be retried through")
        .status();

    assert_eq!(status.as_u16(), 200);
    assert_eq!(site.hits("/x"), 3, "each reset attempt reached the socket");
}
