//! Integration tests that load `test-abi.wasm` through the full WasmRuntime
//! stack and call each WIT export to verify host ABI correctness end-to-end.
//!
//! These tests are **skipped** (with an explanatory message) when the WASM binary
//! has not been built yet. Build it first:
//!
//!   cargo run -p kani-cli -- build --dev
//!
//! Then run the tests:
//!
//!   cargo test -p kani-core --test wasm_abi

#![allow(clippy::unwrap_used)]
#![allow(clippy::pedantic)]

use futures::channel::mpsc;
use kani_core::wasm::WasmRuntime;
use kani_core::wasm::kani::extension::types::ChapterInfo;
use std::pin::Pin;
use std::task::{Context, Poll};
use wasmtime::StoreContextMut;
use wasmtime::component::{Source, StreamConsumer, StreamResult};

fn workspace_root() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().to_path_buf()
}

/// Reads a built extension, failing loudly when it is absent.
///
/// Returning `None` and letting each test return early meant a missing artifact
/// reported passing tests that had executed nothing. CI builds these in the
/// `test` job, so absence is a broken environment, not a reason to pass.
fn load_wasm(name: &str) -> Vec<u8> {
    let path = workspace_root()
        .join("wasm_sources")
        .join(format!("{name}.wasm"));
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "wasm_sources/{name}.wasm is missing ({e}).\n\
             Build it with: cargo run -p kani-cli -- build --dev"
        )
    })
}

/// A generous tick budget so the epoch watchdog never fires in tests
/// (no background thread calls engine.increment_epoch()).
const EPOCH_TICKS: u64 = 50_000;

async fn make_instance(
    bytes: &[u8],
) -> (
    WasmRuntime,
    wasmtime::Store<kani_core::wasm::HostState>,
    kani_core::wasm::KaniExtension,
) {
    let rt = WasmRuntime::new_on_demand().unwrap();
    let component = rt.compile_component(bytes).unwrap();
    let mut store = rt.create_store();
    store.set_epoch_deadline(EPOCH_TICKS);
    let instance = rt.instantiate(&mut store, &component).await.unwrap();
    (rt, store, instance)
}

#[tokio::test]
async fn abi_html_imports_return_extracted_attr_and_text() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_get_popular_manga(&mut store, 1, 20, &[])
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert_eq!(result.manga.len(), 1, "html test should return 1 item");
    let m = &result.manga[0];
    assert_eq!(m.id, "42", "html attr extraction");
    assert_eq!(m.title, "Hello World", "html text extraction");
    assert_eq!(m.cover_url.as_deref(), Some("html-ok"), "html sentinel");
}

#[tokio::test]
async fn abi_json_imports_return_extracted_fields() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_get_popular_manga(&mut store, 2, 20, &[])
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert_eq!(result.manga.len(), 1, "json test should return 1 item");
    let m = &result.manga[0];
    assert_eq!(m.id, "Alice", "json get_str");
    assert_eq!(m.title, "30", "json get_i64");
    assert_eq!(
        m.cover_url.as_deref(),
        Some("json-ok"),
        "json sentinel: active/score/tags/keys all verified"
    );
}

#[tokio::test]
async fn abi_utility_imports_return_decoded_string_and_query_param() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_get_popular_manga(&mut store, 3, 20, &[])
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert_eq!(result.manga.len(), 1, "utility test should return 1 item");
    let m = &result.manga[0];
    assert_eq!(m.id, "util-ok", "utility sentinel");
    assert_eq!(m.title, "hello world", "url_encode+decode round-trip");
    assert_eq!(
        m.cover_url.as_deref(),
        Some("5"),
        "get_query_param from build_url result"
    );
}

#[tokio::test]
async fn abi_prefs_get_value_returns_injected_preferences() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    {
        let data = store.data_mut();
        data.preferences
            .insert("test_str".into(), "injected_value".into());
        data.preferences.insert("test_bool".into(), "true".into());
        data.preferences.insert("test_i64".into(), "42".into());
        data.preferences.insert("test_f64".into(), "3.14".into());
    }

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_search_manga(&mut store, "prefs", 1, 20, &[])
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert_eq!(result.manga.len(), 1, "prefs test should return 1 item");
    let m = &result.manga[0];
    assert_eq!(m.id, "injected_value", "prefs get_str");
    assert_eq!(m.title, "true", "prefs get_bool");
    assert_eq!(
        m.cover_url.as_deref(),
        Some("prefs-ok"),
        "prefs sentinel: raw/missing checks passed"
    );
}

#[tokio::test]
async fn abi_extract_html_returns_rows_from_blueprint() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_search_manga(&mut store, "extract-html", 1, 20, &[])
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert_eq!(result.manga.len(), 2, "extract_html should return 2 rows");
    assert_eq!(result.manga[0].id, "1");
    assert_eq!(result.manga[0].title, "Alpha");
    assert_eq!(result.manga[1].id, "2");
    assert_eq!(result.manga[1].title, "Beta");
}

#[tokio::test]
async fn abi_extract_json_returns_rows_from_blueprint() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_search_manga(&mut store, "extract-json", 1, 20, &[])
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert_eq!(result.manga.len(), 2, "extract_json should return 2 rows");
    assert_eq!(result.manga[0].id, "j1");
    assert_eq!(result.manga[0].title, "JsonAlpha");
    assert_eq!(result.manga[1].id, "j2");
    assert_eq!(result.manga[1].title, "JsonBeta");
}

#[tokio::test]
async fn abi_invalid_handles_return_errors() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_get_manga_details(&mut store, "error-paths")
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert_eq!(
        result.description.as_deref(),
        Some("error-paths-ok"),
        "all invalid-handle ABI calls must return Err"
    );
}

#[tokio::test]
async fn abi_get_metadata_returns_correct_extension_info() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let raw_meta = provider
        .call_get_metadata(&mut store)
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");
    let meta: kani_shared::ExtensionMetadata =
        serde_json::from_str(&raw_meta).expect("metadata is valid JSON");

    assert_eq!(meta.id, "test-abi");
    assert_eq!(meta.name, "TestAbi");
    assert!(!meta.nsfw);
    assert!(!meta.unrestricted_http);
    assert_eq!(meta.language, "en");
}

#[tokio::test]
async fn abi_unknown_page_returns_empty_list() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_get_popular_manga(&mut store, 99, 20, &[])
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert!(result.manga.is_empty());
}

#[tokio::test]
async fn abi_unknown_query_returns_empty_list() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_search_manga(&mut store, "unknown", 1, 20, &[])
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert!(result.manga.is_empty());
}

#[tokio::test]
async fn abi_get_chapter_list_returns_empty() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_get_chapter_list(&mut store, "any", 1, None, None)
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert!(result.chapters.is_empty());
    assert!(!result.has_next_page);
}

#[tokio::test]
async fn abi_get_filter_list_returns_empty() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_get_filter_list(&mut store)
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert!(result.filters.is_empty());
}

#[tokio::test]
async fn abi_get_preferences_returns_empty() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();
    let result = provider
        .call_get_preferences(&mut store)
        .await
        .expect("WASM call failed")
        .expect("extension returned Err");

    assert!(result.is_empty());
}

#[tokio::test]
async fn abi_handles_are_cleaned_up_after_each_call() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let provider = instance.kani_extension_manga_provider();

    provider
        .call_get_popular_manga(&mut store, 1, 20, &[])
        .await
        .unwrap()
        .unwrap();

    let state = store.data();
    assert!(
        state.html_docs.is_empty(),
        "html_docs should be empty after HTML test: {:?}",
        state.html_docs.keys().collect::<Vec<_>>()
    );
    assert!(
        state.html_lists.is_empty(),
        "html_lists should be empty after HTML test"
    );

    provider
        .call_get_popular_manga(&mut store, 2, 20, &[])
        .await
        .unwrap()
        .unwrap();

    assert!(
        store.data().json_docs.is_empty(),
        "json_docs should be empty after JSON test"
    );
}

type ChapterStreamItem =
    Result<ChapterInfo, kani_core::wasm::kani::extension::types::ExtensionError>;

struct CollectConsumer(mpsc::UnboundedSender<ChapterStreamItem>);

impl<D> StreamConsumer<D> for CollectConsumer {
    type Item = ChapterStreamItem;

    fn poll_consume(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        mut source: Source<Self::Item>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let this = self.get_mut();
        let value = &mut None;
        source.read(store, value)?;
        if let Some(v) = value.take() {
            let _ = this.0.unbounded_send(v);
        }
        Poll::Ready(Ok(StreamResult::Completed))
    }
}

async fn drain_chapter_list_stream(
    store: &mut wasmtime::Store<kani_core::wasm::HostState>,
    instance: &kani_core::wasm::KaniExtension,
    manga_id: &str,
) -> Vec<ChapterStreamItem> {
    let provider = instance.kani_extension_manga_provider();
    store
        .run_concurrent(
            async move |accessor| -> wasmtime::Result<Vec<ChapterStreamItem>> {
                let stream = provider
                    .call_get_chapter_list_stream(accessor, manga_id.to_string(), None)
                    .await?;
                let (tx, mut rx) = mpsc::unbounded();
                accessor.with(|store| stream.pipe(store, CollectConsumer(tx)))?;
                let mut items = Vec::new();
                while let Some(v) = futures::StreamExt::next(&mut rx).await {
                    items.push(v);
                }
                Ok(items)
            },
        )
        .await
        .expect("run_concurrent failed")
        .expect("stream drain failed")
}

struct DropAfterFirstConsumer(mpsc::UnboundedSender<ChapterStreamItem>);

impl<D> StreamConsumer<D> for DropAfterFirstConsumer {
    type Item = ChapterStreamItem;

    fn poll_consume(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        mut source: Source<Self::Item>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let this = self.get_mut();
        let value = &mut None;
        source.read(store, value)?;
        if let Some(v) = value.take() {
            let _ = this.0.unbounded_send(v);
            Poll::Ready(Ok(StreamResult::Dropped))
        } else {
            Poll::Ready(Ok(StreamResult::Completed))
        }
    }
}

async fn drain_stream_dropping_after_first(
    store: &mut wasmtime::Store<kani_core::wasm::HostState>,
    instance: &kani_core::wasm::KaniExtension,
    manga_id: &str,
) -> Vec<ChapterStreamItem> {
    let provider = instance.kani_extension_manga_provider();
    store
        .run_concurrent(
            async move |accessor| -> wasmtime::Result<Vec<ChapterStreamItem>> {
                let stream = provider
                    .call_get_chapter_list_stream(accessor, manga_id.to_string(), None)
                    .await?;
                let (tx, mut rx) = mpsc::unbounded();
                accessor.with(|store| stream.pipe(store, DropAfterFirstConsumer(tx)))?;
                let mut items = Vec::new();
                while let Some(v) = futures::StreamExt::next(&mut rx).await {
                    items.push(v);
                }
                Ok(items)
            },
        )
        .await
        .expect("run_concurrent failed")
        .expect("stream drain failed")
}

fn ok_chapters(items: Vec<ChapterStreamItem>) -> Vec<ChapterInfo> {
    items
        .into_iter()
        .map(|r| r.expect("expected Ok chapter item"))
        .collect()
}

#[tokio::test]
async fn abi_get_chapter_list_stream_bridge_yields_nothing_for_empty_sync_list() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let chapters = ok_chapters(drain_chapter_list_stream(&mut store, &instance, "any").await);

    assert!(
        chapters.is_empty(),
        "default bridge over test-abi's empty sync get_chapter_list should yield zero chapters"
    );
}

#[tokio::test]
async fn abi_get_chapter_list_stream_bridge_delivers_all_pages_in_order() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let chapters =
        ok_chapters(drain_chapter_list_stream(&mut store, &instance, "paginated-stream").await);

    let ids: Vec<&str> = chapters.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        chapters.len(),
        4,
        "default bridge should page through get_chapter_list until has_next_page is false, got {ids:?}"
    );
    assert_eq!(
        chapters.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["p1-1", "p1-2", "p2-1", "p2-2"],
        "chapters should arrive in page/within-page order"
    );
}

#[tokio::test]
async fn abi_get_chapter_list_stream_native_override_survives_reentrant_host_call() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let chapters =
        ok_chapters(drain_chapter_list_stream(&mut store, &instance, "native-stream").await);

    assert_eq!(
        chapters.len(),
        2,
        "native override should stream two chapters around a reentrant extraction::extract_html call"
    );
    assert_eq!(chapters[0].id, "native-1");
    assert_eq!(chapters[1].id, "native-2");
}

#[tokio::test]
async fn abi_get_chapter_list_stream_bridge_surfaces_fetch_error_instead_of_swallowing_it() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let items = drain_chapter_list_stream(&mut store, &instance, "error-timeout").await;

    assert_eq!(
        items.len(),
        1,
        "a get_chapter_list error on the first page should surface as exactly one Err item, \
         not be silently swallowed by an empty stream"
    );
    let err = items[0]
        .as_ref()
        .expect_err("expected the stream's only item to be an Err");
    assert_eq!(
        err.kind,
        kani_core::wasm::kani::extension::types::ExtensionErrorKind::Timeout
    );
    assert_eq!(err.message, "request timed out");
}

#[tokio::test]
async fn abi_get_chapter_list_stream_reader_drop_midstream_does_not_hang_or_leak() {
    let bytes = load_wasm("test-abi");
    let (_rt, mut store, instance) = make_instance(&bytes).await;

    let items = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        drain_stream_dropping_after_first(&mut store, &instance, "paginated-stream"),
    )
    .await
    .expect("dropping the stream reader mid-stream must not hang the host");

    assert_eq!(
        items.len(),
        1,
        "dropping the reader after the first chunk should deliver exactly one item"
    );
    assert!(
        items[0].is_ok(),
        "the single delivered item should be a chapter, not an error"
    );

    let state = store.data();
    assert!(
        state.html_docs.is_empty(),
        "html_docs must be released after a mid-stream reader drop"
    );
    assert!(
        state.html_lists.is_empty(),
        "html_lists must be released after a mid-stream reader drop"
    );
    assert!(
        state.json_docs.is_empty(),
        "json_docs must be released after a mid-stream reader drop"
    );
}
