#![allow(clippy::unwrap_used)]

//! Page-set cache coherence. The reader reads a
//! chapter's pages through the service `pages` cache; the quality/downloader
//! path (`candidate_page_urls`) deliberately bypasses that cache to see what the
//! source offers *now*. For an unchanged source the two must agree, and the
//! cached path must be transparent — same content, one fewer origin hit.

mod common;
use common::{insert_manga, insert_source, test_service};

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::service::AppService;
use kani_app::source::{SourceBackend, YamlSource};
use kani_shared::ast::Expr;
use kani_shared_test::origin::{Response, TestOrigin};
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
    ValidatedTotalPages,
};
use kani_yaml::yaml::schema::ResponseType;

const PAGES_HTML: &str = r#"<html><body>
    <div class="page" data-url="https://cdn.example.com/p1.jpg"></div>
    <div class="page" data-url="https://cdn.example.com/p2.jpg"></div>
    <div class="page" data-url="https://cdn.example.com/p3.jpg"></div>
</body></html>"#;

fn pages_endpoint() -> ValidatedEndpoint {
    ValidatedEndpoint {
        route: "/chapter/$chapter_id$".into(),
        method: "GET".into(),
        headers: vec![],
        queries: vec![],
        filter_mapping: vec![],
        filter_format: None,
        response_type: ResponseType::Html,
        container: ".page".into(),
        bindings: vec![],
        fields: vec![ValidatedField {
            name: "url".to_string(),
            source: FieldSource::Blueprint(Expr::Attr {
                target: Box::new(Expr::SelfRef),
                name: "data-url".to_string(),
            }),
            optional: false,
        }],
        scalars: vec![],
        has_next_page: ValidatedHnp::Static(false),
        total_pages: ValidatedTotalPages::None,
        pagination: None,
        composite_id_decodes: vec![],
        then_steps: vec![],
        for_each_steps: vec![],
        via: None,
        page_url: None,
        script_name: None,
        timeout_ms: 10_000,
        auto_scroll: true,
    }
}

async fn wire_pages_source(svc: &AppService, origin: &TestOrigin) -> i64 {
    let source_id = insert_source(&svc.db, "fixture-source").await;
    insert_manga(&svc.db, source_id, "m1", "Fixture Manga").await;
    let ext = ValidatedExtension {
        id: "fixture-source".into(),
        name: "Fixture Source".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        pages: Some(pages_endpoint()),
        ..Default::default()
    };
    svc.sources.insert(
        source_id,
        SourceBackend::Yaml(Box::new(YamlSource::new(
            Arc::new(ext),
            kani_core::http::SmartClient::new(None)
                .unwrap()
                .with_allow_loopback_egress(true),
            Arc::new(kani_core::cache::InMemoryCache::new()),
            "test:".into(),
            HashMap::new(),
            true,
        ))),
    );
    source_id
}

fn urls_from_json(json: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    v.get("pages")
        .and_then(|p| p.as_array())
        .unwrap()
        .iter()
        .filter_map(|p| p.get("url")?.as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn the_reader_and_the_downloader_agree_on_the_page_set() {
    let origin = TestOrigin::start().await;
    origin.set("/chapter/ch-1", Response::html(PAGES_HTML));
    let svc = test_service().await;
    let source_id = wire_pages_source(&svc, &origin).await;

    let reader_json = svc.get_pages(source_id, "m1", "ch-1").await.unwrap();
    let reader_urls = urls_from_json(&reader_json);
    assert_eq!(reader_urls.len(), 3);
    assert_eq!(
        origin.hits("/chapter/ch-1"),
        1,
        "reader hit the origin once"
    );

    let reader_again = svc.get_pages(source_id, "m1", "ch-1").await.unwrap();
    assert_eq!(
        origin.hits("/chapter/ch-1"),
        1,
        "the second reader call was served from the page cache"
    );
    assert_eq!(urls_from_json(&reader_again), reader_urls);

    let backend = svc.sources.get_backend(source_id).unwrap();
    let direct = backend.get_pages("m1", "ch-1").await.unwrap();
    let direct_urls: Vec<String> = direct.pages.iter().map(|p| p.url.clone()).collect();
    assert_eq!(
        origin.hits("/chapter/ch-1"),
        2,
        "the direct read bypassed the cache and hit the origin again"
    );

    assert_eq!(
        reader_urls, direct_urls,
        "the cached reader and the direct downloader read must agree on the page set"
    );
}
