#![allow(clippy::unwrap_used)]

//! A live probe for `capture_page_payload_resilient`. Mock challenges cannot
//! validate browser fingerprint behaviour, so this drives the real path: the
//! solver clears the challenge and captures the payload in that same browser
//! via `kani.capture`.
//!
//! Ignored by default — it needs network and a running Kani-compatible
//! FlareSolverr. Run it with:
//!
//! ```text
//! KANI_LIVE_URL='https://example.org/browse?page=1' \
//! KANI_LIVE_INIT_SCRIPT_FILE=/path/to/capture_browse.js \
//! KANI_LIVE_SOLVER_URL=http://127.0.0.1:8191/v1 \
//! cargo test -p kani-app --test live_browser_capture_tests -- --ignored --nocapture
//! ```

use kani_core::http::SmartClient;
use kani_core::v8_process;

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the live probe"))
}

#[tokio::test]
#[ignore = "live probe: needs network, Chromium, and a Kani-compatible solver"]
async fn a_managed_challenge_falls_back_to_the_solver_and_yields_a_payload() {
    let url = required("KANI_LIVE_URL");
    let script_file = required("KANI_LIVE_INIT_SCRIPT_FILE");
    let solver_url = required("KANI_LIVE_SOLVER_URL");
    let init_script = std::fs::read_to_string(&script_file).unwrap();

    let handle = v8_process::new_handle();
    let http = SmartClient::new(Some(solver_url)).unwrap();

    let first_started = std::time::Instant::now();
    let payload = v8_process::capture_page_payload_resilient(
        &handle,
        &http,
        &url,
        &init_script,
        45_000,
        Some("live-probe"),
        false,
    )
    .await
    .expect("the challenged page must still yield a payload via the solver");

    println!(
        "cold capture: {} bytes in {:?}",
        payload.len(),
        first_started.elapsed()
    );
    assert!(!payload.is_empty(), "the capture returned an empty payload");
    serde_json::from_str::<serde_json::Value>(&payload)
        .expect("the captured payload must be the site's JSON, not a challenge page");

    let warm_started = std::time::Instant::now();
    let warm_payload = v8_process::capture_page_payload_resilient(
        &handle,
        &http,
        &url,
        &init_script,
        45_000,
        Some("live-probe"),
        false,
    )
    .await
    .expect("the cleared solver session must capture a second payload");
    println!(
        "warm capture: {} bytes in {:?}",
        warm_payload.len(),
        warm_started.elapsed()
    );
    serde_json::from_str::<serde_json::Value>(&warm_payload)
        .expect("the warm capture must return the site's JSON");

    assert_eq!(http.destroy_solver_sessions("live-probe").await, 1);
}
