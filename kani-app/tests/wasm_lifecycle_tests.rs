#![allow(clippy::unwrap_used)]

//! WASM source lifecycle against the compiled
//! `kani-fixture-source`, which makes real HTTP against a `TestOrigin`. Covers the
//! lease/drain coordination and a live error path. Requires the fixture:
//!   cargo run -p kani-cli -- build kani-fixture-source

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kani_app::source::{SourceBackend, SourceRegistry, WasmSource, loader};
use kani_core::cache::InMemoryCache;
use kani_core::http::SmartClient;
use kani_core::wasm::WasmRuntime;
use kani_shared_test::origin::{Response, TestOrigin};

fn fixture_wasm_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("wasm_sources")
        .join("fixture.wasm")
}

/// Build a compiled-WASM backend from `fixture.wasm`, pointed at `origin_base`.
/// Returns `None` (with a skip note) if the fixture has not been built.
fn wasm_backend(origin_base: &str) -> Option<SourceBackend> {
    let path = fixture_wasm_path();
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "\n[SKIP] {} not found — build it with: \
             cargo run -p kani-cli -- build kani-fixture-source\n",
            path.display()
        );
        return None;
    };
    let rt = WasmRuntime::new_on_demand().unwrap();
    let component = rt.compile_component(&bytes).unwrap();
    let instance_pre = rt.instantiate_pre(&component).unwrap();
    let engine = rt.engine().clone();

    let mut prefs = HashMap::new();
    prefs.insert("base_url".to_string(), origin_base.to_string());

    Some(loader::build_wasm_source(
        engine,
        instance_pre,
        SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true),
        None,
        true,
        false,
        prefs,
        Arc::new(InMemoryCache::new()),
        "fixture:".to_string(),
        None,
        None,
        0,
    ))
}

fn as_wasm(backend: &SourceBackend) -> &WasmSource {
    match backend {
        SourceBackend::Wasm(w) => w,
        _ => panic!("expected a WASM backend"),
    }
}

#[tokio::test]
async fn drain_waits_for_a_live_lease_then_forces_after_the_timeout() {
    let origin = TestOrigin::start().await;
    let Some(backend) = wasm_backend(&origin.base()) else {
        return;
    };
    let w = as_wasm(&backend);

    let lease = w.lease_instance().await.unwrap();
    let start = Instant::now();
    w.drain(Duration::from_millis(300)).await;
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(250),
        "drain returned before its timeout while a lease was live ({elapsed:?})"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "drain did not honour its timeout ({elapsed:?})"
    );
    drop(lease);
}

#[tokio::test]
async fn a_lease_is_rejected_once_the_source_is_draining() {
    let origin = TestOrigin::start().await;
    let Some(backend) = wasm_backend(&origin.base()) else {
        return;
    };
    let w = as_wasm(&backend);

    w.drain(Duration::from_millis(50)).await;

    match w.lease_instance().await {
        Err(e) => assert!(
            e.to_string().to_lowercase().contains("updat"),
            "a lease during drain reports the source is updating, got: {e}"
        ),
        Ok(_) => panic!("expected the lease to be rejected while draining"),
    }
}

#[tokio::test]
async fn hot_swap_waits_for_an_in_flight_lease_then_installs_the_new_backend() {
    let origin = TestOrigin::start().await;
    let (Some(old), Some(new)) = (wasm_backend(&origin.base()), wasm_backend(&origin.base()))
    else {
        return;
    };
    let registry = Arc::new(SourceRegistry::default());
    registry.insert(1, old);

    let backend_arc = registry.get_backend(1).unwrap();
    let lease = as_wasm(&backend_arc).lease_instance().await.unwrap();

    let reg2 = registry.clone();
    let swap = tokio::spawn(async move { reg2.hot_swap(1, new).await });

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!swap.is_finished(), "hot_swap swapped out a live lease");

    drop(lease);
    tokio::time::timeout(Duration::from_secs(5), swap)
        .await
        .expect("hot_swap must complete once the lease is released")
        .unwrap();
    assert!(registry.contains_key(1), "the new backend is installed");
}

#[tokio::test]
async fn a_wasm_guest_surfaces_an_upstream_failure() {
    let origin = TestOrigin::start().await;
    origin.set("/popular", Response::status(500));
    let Some(backend) = wasm_backend(&origin.base()) else {
        return;
    };

    let res = backend.get_popular_manga(1, 20, &[]).await;
    assert!(
        res.is_err(),
        "a 500 from the origin surfaces as an error, not an empty list"
    );
}
