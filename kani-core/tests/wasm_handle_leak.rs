//! Cross-boundary handles acquired before a guest failure must be released while
//! unwinding so repeated failures cannot exhaust the host handle table.
//!
//! Build the fixture first:  cargo run -p kani-cli -- build --dev

#![allow(clippy::unwrap_used)]
#![allow(clippy::pedantic)]

use kani_core::wasm::WasmRuntime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const EPOCH_TICKS: u64 = 50_000;
const ITEM_HTML: &str = r#"<html><body><div class="item" data-id="x"></div></body></html>"#;

fn load_fixture() -> Option<Vec<u8>> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("wasm_sources")
        .join("fixture.wasm");
    std::fs::read(&path).ok().or_else(|| {
        eprintln!(
            "\n[SKIP] wasm_sources/fixture.wasm not found.\n\
             Build it with: cargo run -p kani-cli -- build --dev\n"
        );
        None
    })
}

/// Minimal always-200 origin. kani-core's tests cannot use `kani-shared-test`
/// (that crate depends on kani-app, which depends on kani-core), so the few
/// lines are written out here rather than shared.
async fn start_origin() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = stream.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{}",
                    ITEM_HTML.len(),
                    ITEM_HTML
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
async fn a_handle_is_not_leaked_when_a_live_fetch_fails() {
    let Some(bytes) = load_fixture() else { return };
    let base = start_origin().await;

    let rt = WasmRuntime::new_on_demand().unwrap();
    let component = rt.compile_component(&bytes).unwrap();
    let mut store = rt.create_store();
    store.set_epoch_deadline(EPOCH_TICKS);
    store
        .data_mut()
        .preferences
        .insert("base_url".to_string(), base);

    let instance = rt.instantiate(&mut store, &component).await.unwrap();
    let provider = instance.kani_extension_manga_provider();

    let result = provider
        .call_search_manga(&mut store, "__fanout__:40", 1, 20, &[])
        .await
        .expect("the WASM call itself must not trap");
    assert!(
        result.is_err(),
        "the fan-out must exceed the per-call budget, otherwise this test proves nothing"
    );

    let state = store.data();
    assert!(
        state.html_docs.is_empty(),
        "html_docs leaked across a failed call: {:?}",
        state.html_docs.keys().collect::<Vec<_>>()
    );
    assert!(
        state.html_lists.is_empty(),
        "html_lists leaked across a failed call"
    );
    assert!(
        state.json_docs.is_empty(),
        "json_docs leaked across a failed call: {:?}",
        state.json_docs.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn handles_do_not_accumulate_across_repeated_failures() {
    let Some(bytes) = load_fixture() else { return };
    let base = start_origin().await;

    let rt = WasmRuntime::new_on_demand().unwrap();
    let component = rt.compile_component(&bytes).unwrap();
    let mut store = rt.create_store();
    store.set_epoch_deadline(EPOCH_TICKS);
    store
        .data_mut()
        .preferences
        .insert("base_url".to_string(), base);

    let instance = rt.instantiate(&mut store, &component).await.unwrap();
    let provider = instance.kani_extension_manga_provider();

    for _ in 0..5 {
        let _ = provider
            .call_search_manga(&mut store, "__fanout__:40", 1, 20, &[])
            .await
            .expect("the WASM call itself must not trap");
        store.data_mut().io_count = 0;
    }

    let state = store.data();
    let total = state.html_docs.len() + state.html_lists.len() + state.json_docs.len();
    assert_eq!(
        total, 0,
        "handles accumulated over repeated failures — {} still held",
        total
    );
}
