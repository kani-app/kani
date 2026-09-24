#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_manga, insert_source, test_service};

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::events::AppEvent;
use kani_app::source::{SourceBackend, YamlSource};
use kani_shared::ast::Expr;
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
    ValidatedTotalPages,
};
use kani_yaml::yaml::schema::ResponseType;

async fn start_html_server(html: &'static str) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = stream.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    port
}

fn text_field(name: &str, selector: &str) -> ValidatedField {
    ValidatedField {
        name: name.to_string(),
        source: FieldSource::Blueprint(Expr::Text {
            target: Box::new(Expr::First {
                target: Box::new(Expr::SelfRef),
                selector: selector.to_string(),
            }),
        }),
        optional: false,
    }
}

fn self_attr_field(name: &str, attr: &str) -> ValidatedField {
    ValidatedField {
        name: name.to_string(),
        source: FieldSource::Blueprint(Expr::Attr {
            target: Box::new(Expr::SelfRef),
            name: attr.to_string(),
        }),
        optional: false,
    }
}

fn chapter_list_endpoint(route: &str, container: &str) -> ValidatedEndpoint {
    ValidatedEndpoint {
        route: route.to_string(),
        method: "GET".into(),
        headers: vec![],
        queries: vec![],
        filter_mapping: vec![],
        filter_format: None,
        response_type: ResponseType::Html,
        container: container.to_string(),
        bindings: vec![],
        fields: vec![
            self_attr_field("id", "data-id"),
            text_field("title", ".title"),
        ],
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

fn yaml_source(base_url: &str, chapter_ep: ValidatedEndpoint) -> YamlSource {
    let cache = Arc::new(kani_core::cache::InMemoryCache::new());
    let http = kani_core::http::SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true);
    let ext = ValidatedExtension {
        id: "fixture-source".into(),
        name: "Fixture Source".into(),
        version: "1.0.0".into(),
        base_url: base_url.to_string(),
        language: "en".into(),
        unrestricted_http: true,
        chapter_list: Some(chapter_ep),
        ..Default::default()
    };
    YamlSource::new(
        Arc::new(ext),
        http,
        cache,
        "test:".into(),
        HashMap::new(),
        true,
    )
}

#[tokio::test]
async fn scan_for_new_chapters_emits_partial_then_complete_with_correct_counts() {
    let html: &'static str = r#"<html><body>
        <div class="ch" data-id="ch-1"><span class="title">Chapter 1</span></div>
        <div class="ch" data-id="ch-2"><span class="title">Chapter 2</span></div>
    </body></html>"#;

    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "fixture-source").await;
    let manga_id = insert_manga(&svc.db, source_id, "manga-1", "Fixture Manga").await;

    svc.sources.insert(
        source_id,
        SourceBackend::Yaml(Box::new(yaml_source(
            &base_url,
            chapter_list_endpoint("/manga/$manga_id$/chapters", ".ch"),
        ))),
    );

    let mut rx = svc.subscribe_refresh();

    let new_ids = svc.scan_for_new_chapters(manga_id).await.unwrap();
    assert_eq!(new_ids.len(), 2, "both chapters should be newly inserted");

    let partial = rx.try_recv().expect("ChapterListPartial should have fired");
    assert_eq!(
        partial,
        AppEvent::ChapterListPartial {
            manga_id,
            received: 2,
        },
        "partial event should report the correct running total after the only page"
    );

    let complete = rx
        .try_recv()
        .expect("ChapterListComplete should have fired");
    assert_eq!(
        complete,
        AppEvent::ChapterListComplete { manga_id, total: 2 },
        "complete event should report the correct final total"
    );

    let new_chapters = rx.try_recv().expect("NewChapters should have fired");
    assert!(
        matches!(new_chapters, AppEvent::NewChapters { manga_id: id, count: 2, .. } if id == manga_id)
    );
}

#[tokio::test]
async fn scan_for_new_chapters_emits_error_event_on_fetch_failure() {
    let dead_port = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };
    let base_url = format!("http://127.0.0.1:{dead_port}");

    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "fixture-source").await;
    let manga_id = insert_manga(&svc.db, source_id, "manga-1", "Fixture Manga").await;

    svc.sources.insert(
        source_id,
        SourceBackend::Yaml(Box::new(yaml_source(
            &base_url,
            chapter_list_endpoint("/manga/$manga_id$/chapters", ".ch"),
        ))),
    );

    let mut rx = svc.subscribe_refresh();

    let result = svc.scan_for_new_chapters(manga_id).await;
    assert!(result.is_err(), "fetch against a dead server should fail");

    let event = rx.try_recv().expect("ChapterListError should have fired");
    match event {
        AppEvent::ChapterListError {
            manga_id: id,
            error,
        } => {
            assert_eq!(id, manga_id);
            assert!(!error.is_empty());
        }
        other => panic!("expected ChapterListError, got {other:?}"),
    }
}

#[tokio::test]
async fn fetch_and_store_chapters_silent_emits_no_stream_events() {
    let html: &'static str = r#"<html><body>
        <div class="ch" data-id="ch-1"><span class="title">Chapter 1</span></div>
    </body></html>"#;

    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "fixture-source").await;
    let manga_id = insert_manga(&svc.db, source_id, "manga-1", "Fixture Manga").await;

    svc.sources.insert(
        source_id,
        SourceBackend::Yaml(Box::new(yaml_source(
            &base_url,
            chapter_list_endpoint("/manga/$manga_id$/chapters", ".ch"),
        ))),
    );

    let mut rx = svc.subscribe_refresh();

    let new_ids = svc.fetch_and_store_chapters_silent(manga_id).await.unwrap();
    assert_eq!(new_ids.len(), 1);

    // The silent path must not broadcast anything at all — bulk import relies on this
    // to avoid spamming notifications across hundreds of manga.
    assert!(
        rx.try_recv().is_err(),
        "fetch_and_store_chapters_silent must not emit any SSE events"
    );
}
