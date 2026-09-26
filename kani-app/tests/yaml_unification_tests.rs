#![allow(clippy::unwrap_used)]
#![allow(clippy::pedantic)]

//! Requires both engines that consume the same YAML to produce equivalent behavior.
//!
//!   path 1  YAML → kani-cli codegen → WASM component  (`fixture-gen.wasm`)
//!   path 2  YAML → ValidatedExtension → interpreted `YamlSource`
//!
//! Both are pointed at one `TestOrigin` and driven through the shared
//! `SourceBackend` interface. We assert the request each puts on the wire and the
//! parsed result agree. Divergence indicates that one engine reads the specification differently.
//!
//! Requires the codegen'd fixture: `cargo run -p kani-cli -- build --dev`.

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::source::{SourceBackend, loader};
use kani_core::cache::InMemoryCache;
use kani_core::http::SmartClient;
use kani_core::wasm::WasmRuntime;
use kani_shared::types::{ActiveFilter, FilterState};
use kani_shared_test::origin::{Response, TestOrigin};

const FIXTURE_YAML: &str = include_str!("../../fixture-gen.yaml");

const ITEMS_HTML: &str = r#"<html><body>
    <div class="item" data-id="manga-1"><span class="title">First</span></div>
    <div class="item" data-id="manga-2"><span class="title">Second</span></div>
</body></html>"#;

const DETAILS_HTML: &str = r#"<html><body>
    <div class="manga" data-id="manga-1"><h1>My Title</h1></div>
</body></html>"#;

const CHAPTERS_HTML: &str = r#"<html><body>
    <div class="ch" data-id="ch-1"><span class="title">Chapter 1</span></div>
    <div class="ch" data-id="ch-2"><span class="title">Chapter 2</span></div>
</body></html>"#;

const PAGES_HTML: &str = r#"<html><body>
    <div class="page" data-url="https://cdn.example.com/p1.jpg"></div>
    <div class="page" data-url="https://cdn.example.com/p2.jpg"></div>
</body></html>"#;

fn seed(origin: &TestOrigin) {
    origin.set("/popular", Response::html(ITEMS_HTML));
    origin.set("/search", Response::html(ITEMS_HTML));
    origin.set("/manga/manga-1", Response::html(DETAILS_HTML));
    origin.set("/manga/manga-1/chapters", Response::html(CHAPTERS_HTML));
    origin.set("/manga/manga-1/chapter/ch-1", Response::html(PAGES_HTML));
}

/// path 2 — interpreted. Parse the fixture YAML, retarget its base_url at the
/// origin, wrap in a `YamlSource`.
fn interpreted(origin_base: &str) -> SourceBackend {
    let mut config =
        kani_yaml::parse_and_validate(FIXTURE_YAML, std::path::Path::new("fixture-gen.yaml"))
            .expect("fixture-gen.yaml must validate");
    config.base_url = origin_base.to_string();
    loader::build_yaml_source(
        Arc::new(config),
        SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true),
        Arc::new(InMemoryCache::new()),
        "fixture-gen:".into(),
        HashMap::new(),
        false,
    )
}

/// path 1 — compiled. Load the codegen'd component, inject the origin as the
/// `base_url` preference its (generated) request-builders read.
fn compiled(origin_base: &str) -> Option<SourceBackend> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("wasm_sources")
        .join("fixture-gen.wasm");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "\n[SKIP] {} not found — build it with: cargo run -p kani-cli -- build --dev\n",
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
        "fixture-gen:".to_string(),
        None,
        None,
        0,
    ))
}

macro_rules! compiled_or_skip {
    ($origin:expr) => {
        match compiled(&$origin.base()) {
            Some(b) => b,
            None => return,
        }
    };
}

fn genre_filter() -> ActiveFilter {
    ActiveFilter {
        filter_name: "genre".to_string(),
        state: FilterState::Multiselect(vec!["action".to_string()]),
    }
}

#[tokio::test]
async fn popular_result_parity() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());

    let wr = w.get_popular_manga(1, 20, &[]).await.unwrap();
    let yr = y.get_popular_manga(1, 20, &[]).await.unwrap();
    let wids: Vec<_> = wr.manga.iter().map(|m| (&m.id, &m.title)).collect();
    let yids: Vec<_> = yr.manga.iter().map(|m| (&m.id, &m.title)).collect();
    assert_eq!(wids, yids);
    assert_eq!(wids.len(), 2);
}

#[tokio::test]
async fn details_result_parity() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());

    let wr = w.get_manga_details("manga-1").await.unwrap();
    let yr = y.get_manga_details("manga-1").await.unwrap();
    assert_eq!((&wr.id, &wr.title), (&yr.id, &yr.title));
    assert_eq!(wr.title, "My Title");
}

#[tokio::test]
async fn chapter_list_result_parity() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());

    let wr = w.get_chapter_list("manga-1", 1, None, None).await.unwrap();
    let yr = y.get_chapter_list("manga-1", 1, None, None).await.unwrap();
    let wids: Vec<_> = wr.chapters.iter().map(|c| &c.id).collect();
    let yids: Vec<_> = yr.chapters.iter().map(|c| &c.id).collect();
    assert_eq!(wids, yids);
    assert_eq!(wids, vec!["ch-1", "ch-2"]);
}

#[tokio::test]
async fn pages_result_parity() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());

    let wr = w.get_pages("manga-1", "ch-1").await.unwrap();
    let yr = y.get_pages("manga-1", "ch-1").await.unwrap();
    let wu: Vec<_> = wr.pages.iter().map(|p| (p.index, &p.url)).collect();
    let yu: Vec<_> = yr.pages.iter().map(|p| (p.index, &p.url)).collect();
    assert_eq!(wu, yu);
    assert_eq!(wr.pages.len(), 2);
}

#[tokio::test]
async fn search_request_parity_with_filter_and_pagination() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());
    let filters = [genre_filter()];

    y.search_manga("naruto", 1, 20, &filters).await.unwrap();
    let y_req = origin
        .last_request("/search")
        .expect("interpreted hit /search");
    let y_q = y_req.query_param("q");
    let y_genre = y_req.query_param("genre");
    let y_page = y_req.query_param("page");

    w.search_manga("naruto", 1, 20, &filters).await.unwrap();
    let w_req = origin
        .last_request("/search")
        .expect("compiled hit /search");

    assert_eq!(w_req.query_param("q"), y_q, "query param q must match");
    assert_eq!(
        w_req.query_param("genre"),
        y_genre,
        "filter mapping (genre) must match across engines (A1)"
    );
    assert_eq!(
        w_req.query_param("page"),
        y_page,
        "pagination offset param must match across engines"
    );
    assert_eq!(y_genre.as_deref(), Some("action"));
}

#[tokio::test]
async fn details_request_path_parity() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());

    y.get_manga_details("manga-1").await.unwrap();
    let y_path = origin.last_request("/manga/manga-1").map(|r| r.path);
    w.get_manga_details("manga-1").await.unwrap();
    let w_path = origin.last_request("/manga/manga-1").map(|r| r.path);
    assert_eq!(w_path, y_path);
    assert_eq!(w_path.as_deref(), Some("/manga/manga-1"));
}
