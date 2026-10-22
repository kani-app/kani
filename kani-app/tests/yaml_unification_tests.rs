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

    let wr = w
        .get_chapter_list("manga-1", 1, Some(2), None)
        .await
        .unwrap();
    let yr = y
        .get_chapter_list("manga-1", 1, Some(2), None)
        .await
        .unwrap();
    let wids: Vec<_> = wr.chapters.iter().map(|c| &c.id).collect();
    let yids: Vec<_> = yr.chapters.iter().map(|c| &c.id).collect();
    assert_eq!(wids, yids);
    assert_eq!(wids, vec!["ch-1", "ch-2"]);

    y.get_chapter_list("manga-1", 1, None, Some("date_desc".into()))
        .await
        .unwrap();
    let y_order = origin
        .last_request("/manga/manga-1/chapters")
        .unwrap()
        .query_param("order");
    w.get_chapter_list("manga-1", 1, None, Some("date_desc".into()))
        .await
        .unwrap();
    let w_order = origin
        .last_request("/manga/manga-1/chapters")
        .unwrap()
        .query_param("order");
    assert_eq!(
        y_order.as_deref(),
        Some("date_desc"),
        "the selected sort reaches the source"
    );
    assert_eq!(w_order, y_order);
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
    assert_eq!(
        y_req.query_param("kw").as_deref(),
        Some("title-naruto+extra")
    );
    assert_eq!(w_req.query_param("kw"), y_req.query_param("kw"));
}

#[tokio::test]
async fn details_request_path_parity() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());

    y.get_manga_details("manga-1").await.unwrap();
    let y_req = origin.last_request("/manga/manga-1").unwrap();
    w.get_manga_details("manga-1").await.unwrap();
    let w_req = origin.last_request("/manga/manga-1").unwrap();
    assert_eq!(w_req.path, y_req.path);
    assert_eq!(w_req.path, "/manga/manga-1");
    assert_eq!(y_req.query_param("ref").as_deref(), Some("id-manga-1"));
    assert_eq!(w_req.query_param("ref"), y_req.query_param("ref"));
}

#[tokio::test]
async fn awkward_ids_are_encoded_identically_on_the_wire() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());
    let id = "a b/&?#%é%2F";
    let path = "/manga/a%20b%2F%26%3F%23%25%C3%A9%252F";

    let _ = y.get_manga_details(id).await;
    let y_req = origin
        .last_request(path)
        .expect("interpreted sent the encoded path");
    let _ = w.get_manga_details(id).await;
    assert_eq!(origin.hits(path), 2, "compiled sent the same encoded path");
    let w_req = origin.last_request(path).unwrap();
    assert_eq!(
        y_req.query_param("ref").as_deref(),
        Some("id-a+b%2F%26%3F%23%25%C3%A9%252F")
    );
    assert_eq!(w_req.query_param("ref"), y_req.query_param("ref"));

    let header_id = "m 1/&?#%2F";
    let pages_path = "/manga/m%201%2F%26%3F%23%252F/chapter/ch-1";
    let _ = y.get_pages(header_id, "ch-1").await;
    let y_req = origin
        .last_request(pages_path)
        .expect("interpreted pages request");
    let _ = w.get_pages(header_id, "ch-1").await;
    assert_eq!(
        origin.hits(pages_path),
        2,
        "compiled sent the same encoded path"
    );
    let w_req = origin.last_request(pages_path).unwrap();
    assert_eq!(
        y_req.header("x-ref"),
        Some("m-m 1/&?#%2F"),
        "headers are interpolated as text"
    );
    assert_eq!(w_req.header("x-ref"), y_req.header("x-ref"));

    for backend in [&y, &w] {
        let err = backend
            .get_pages("x\r\nX-Evil: 1", "ch-1")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("CR or LF"), "{err}");
    }
}

#[tokio::test]
async fn request_bodies_are_byte_identical_on_both_backends() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());
    let query = "say \"hi\" \\ back\nnext é 🐉";

    y.search_manga(query, 1, 20, &[]).await.unwrap();
    let y_req = origin.last_request("/search").unwrap();
    w.search_manga(query, 1, 20, &[]).await.unwrap();
    let w_req = origin.last_request("/search").unwrap();

    assert_eq!(y_req.method, "POST");
    assert_eq!(y_req.header("content-type"), Some("application/json"));
    assert_eq!(
        String::from_utf8(y_req.body.clone()).unwrap(),
        r#"{"label":"p1","page":1,"q":"say \"hi\" \\ back\nnext é 🐉"}"#
    );
    assert_eq!(
        w_req.body, y_req.body,
        "the compiled backend sends the same bytes"
    );
    assert_eq!(w_req.header("content-type"), y_req.header("content-type"));
}

#[tokio::test]
async fn a_chapter_list_spanning_two_native_chunks_matches_on_both_backends() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());
    let last = r#"<html><body><div class="ch" data-id="ch-3"><span class="title">Chapter 3</span></div></body></html>"#;
    let chunks = || vec![Response::html(CHAPTERS_HTML), Response::html(last)];

    let mut results = Vec::new();
    for backend in [&y, &w] {
        origin.script("/manga/manga-1/chapters", chunks());
        let list = backend
            .get_chapter_list("manga-1", 1, Some(3), None)
            .await
            .unwrap();
        let seen = origin.last_request("/manga/manga-1/chapters").unwrap();
        results.push((
            list.chapters
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>(),
            list.has_next_page,
            seen.query_param("p"),
        ));
    }
    assert_eq!(
        results[0].0,
        ["ch-1", "ch-2", "ch-3"],
        "both chunks were read"
    );
    assert!(!results[0].1, "the short second chunk ends the list");
    assert_eq!(
        results[0].2.as_deref(),
        Some("2"),
        "the second chunk was requested"
    );
    assert_eq!(
        results[1], results[0],
        "the compiled backend pages the same way"
    );
}

fn chapter_chunk(ids: &[&str], last_native_page: u32) -> Response {
    let rows: String = ids
        .iter()
        .map(|id| {
            format!(r#"<div class="ch" data-id="{id}"><span class="title">{id}</span></div>"#)
        })
        .collect();
    Response::html(&format!(
        r#"<html><body>{rows}<div class="pager" data-last="{last_native_page}"></div></body></html>"#
    ))
}

#[tokio::test]
async fn a_declared_total_pages_expression_reaches_the_client_on_both_backends() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());

    let mut results = Vec::new();
    for backend in [&y, &w] {
        origin.script(
            "/manga/manga-1/chapters",
            vec![chapter_chunk(&["ch-1", "ch-2"], 5)],
        );
        let list = backend
            .get_chapter_list("manga-1", 1, Some(2), None)
            .await
            .unwrap();
        results.push((list.total_pages, list.has_next_page));
    }
    assert_eq!(
        results[0],
        (Some(5), true),
        "the source's page count is the declared expression's value"
    );
    assert_eq!(results[1], results[0], "the compiled backend agrees");
}

#[tokio::test]
async fn a_short_final_chunk_makes_the_page_count_exact_on_both_backends() {
    let origin = TestOrigin::start().await;
    seed(&origin);
    let w = compiled_or_skip!(origin);
    let y = interpreted(&origin.base());

    let mut results = Vec::new();
    for backend in [&y, &w] {
        origin.script("/manga/manga-1/chapters", vec![chapter_chunk(&["ch-3"], 2)]);
        let list = backend
            .get_chapter_list("manga-1", 3, Some(1), None)
            .await
            .unwrap();
        let seen = origin.last_request("/manga/manga-1/chapters").unwrap();
        results.push((
            list.chapters
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>(),
            list.total_pages,
            list.has_next_page,
            seen.query_param("p"),
        ));
    }
    assert_eq!(
        results[0],
        (
            vec!["ch-3".to_string()],
            Some(3),
            false,
            Some("2".to_string())
        ),
        "two native pages of two could hold four chapters, but the second held one: \
         three pages of one, and page 3 is the last"
    );
    assert_eq!(results[1], results[0], "the compiled backend agrees");
}
