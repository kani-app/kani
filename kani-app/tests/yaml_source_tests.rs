#![allow(clippy::unwrap_used)]

mod common;
use common::test_service;

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::source::{SourceBackend, SourceRegistry, YamlSource};
use kani_shared::ast::Expr;
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
    ValidatedPopular, ValidatedTotalPages,
};
use kani_yaml::yaml::schema::{
    FilterEntry, FilterKind, FilterOption as SchemaFilterOption, FilterSemantic, PreferenceEntry,
    PreferenceKind, ResponseType,
};

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

fn list_endpoint(route: &str, container: &str) -> ValidatedEndpoint {
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

fn yaml_source(_base_url: &str, ext: ValidatedExtension) -> YamlSource {
    yaml_source_with_browser(ext, true)
}

fn yaml_source_with_browser(ext: ValidatedExtension, browser_enabled: bool) -> YamlSource {
    let cache = Arc::new(kani_core::cache::InMemoryCache::new());
    let http = kani_core::http::SmartClient::new(None).unwrap();
    YamlSource::new(
        Arc::new(ext),
        http,
        cache,
        "test:".into(),
        HashMap::new(),
        browser_enabled,
    )
}

#[tokio::test]
async fn popular_manga_extracts_items_from_html() {
    let html: &'static str = r#"<html><body>
        <div class="item" data-id="manga-1"><span class="title">My Manga</span></div>
        <div class="item" data-id="manga-2"><span class="title">Another Manga</span></div>
    </body></html>"#;

    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            popular: Some(ValidatedPopular::Full(Box::new(list_endpoint(
                "/popular", ".item",
            )))),
            ..Default::default()
        },
    );

    let result = src.get_popular_manga(1, 20, &[]).await.unwrap();

    assert_eq!(result.manga.len(), 2);
    assert_eq!(result.manga[0].id, "manga-1");
    assert_eq!(result.manga[0].title, "My Manga");
    assert_eq!(result.manga[1].id, "manga-2");
    assert_eq!(result.manga[1].title, "Another Manga");
    assert!(!result.has_next_page);
}

#[tokio::test]
async fn search_manga_extracts_items_from_html() {
    let html: &'static str = r#"<html><body>
        <div class="item" data-id="s-1"><span class="title">Search Hit</span></div>
    </body></html>"#;

    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            search: Some(list_endpoint("/search", ".item")),
            ..Default::default()
        },
    );

    let result = src.search_manga("Search Hit", 1, 20, &[]).await.unwrap();

    assert_eq!(result.manga.len(), 1);
    assert_eq!(result.manga[0].id, "s-1");
    assert_eq!(result.manga[0].title, "Search Hit");
}

#[tokio::test]
async fn manga_details_extracts_title_and_id() {
    let html: &'static str = r#"<html><body>
        <div class="manga" data-id="manga-42"><h1>My Title</h1></div>
    </body></html>"#;

    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let details_ep = ValidatedEndpoint {
        route: "/manga/$manga_id$".into(),
        fields: vec![self_attr_field("id", "data-id"), text_field("title", "h1")],
        container: ".manga".into(),
        ..list_endpoint("/manga/$manga_id$", ".manga")
    };

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            manga_details: Some(details_ep),
            ..Default::default()
        },
    );

    let result = src.get_manga_details("manga-42").await.unwrap();

    assert_eq!(result.id, "manga-42");
    assert_eq!(result.title, "My Title");
}

#[tokio::test]
async fn delegated_popular_does_not_deadlock() {
    let html: &'static str = r#"<html><body>
        <div class="item" data-id="x-1"><span class="title">Delegated</span></div>
    </body></html>"#;

    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            popular: Some(ValidatedPopular::Delegated {
                delegate_to: "search".into(),
                empty_without_filters: false,
            }),
            search: Some(list_endpoint("/search", ".item")),
            ..Default::default()
        },
    );

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        src.get_popular_manga(1, 20, &[]),
    )
    .await
    .expect("timed out — semaphore double-acquire deadlock")
    .unwrap();

    assert_eq!(result.manga.len(), 1);
    assert_eq!(result.manga[0].id, "x-1");
}

#[tokio::test]
async fn empty_without_filters_returns_empty_list_without_http() {
    let src = yaml_source(
        "http://127.0.0.1:1",
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: "http://127.0.0.1:1".into(),
            language: "en".into(),
            unrestricted_http: true,
            popular: Some(ValidatedPopular::Delegated {
                delegate_to: "search".into(),
                empty_without_filters: true,
            }),
            ..Default::default()
        },
    );

    let result = src.get_popular_manga(1, 20, &[]).await.unwrap();
    assert!(result.manga.is_empty());
    assert!(!result.has_next_page);
}

#[test]
fn capability_mismatch_produces_load_error() {
    let result = kani_app::install_gating::check_required_capabilities(
        &["nonexistent_capability".to_string()],
        kani_core::http::SolverCapability::Capture,
    );
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("nonexistent_capability"));
}

#[test]
fn known_capabilities_are_all_accepted() {
    let caps: Vec<String> = [
        "unrestricted_http",
        "browser_payload",
        "rhai_scripting",
        "scoped_cache",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert!(
        kani_app::install_gating::check_required_capabilities(
            &caps,
            kani_core::http::SolverCapability::Capture,
        )
        .is_ok()
    );
}

#[test]
fn capability_unrestricted_http_is_supported() {
    let result = kani_app::install_gating::check_required_capabilities(
        &["unrestricted_http".to_string()],
        kani_core::http::SolverCapability::NotConfigured,
    );
    assert!(result.is_ok());
}

#[tokio::test]
async fn metadata_serialises_from_config() {
    let cache = Arc::new(kani_core::cache::InMemoryCache::new());
    let http = kani_core::http::SmartClient::new(None).unwrap();
    let src = YamlSource::new(
        Arc::new(ValidatedExtension {
            id: "test-id".into(),
            name: "Test Source".into(),
            ..Default::default()
        }),
        http,
        cache,
        String::new(),
        HashMap::new(),
        true,
    );
    let meta_json = src.get_metadata().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&meta_json).unwrap();
    assert_eq!(parsed["id"].as_str().unwrap(), "test-id");
    assert_eq!(parsed["name"].as_str().unwrap(), "Test Source");
}

#[tokio::test]
async fn chapter_list_extracts_chapters_from_html() {
    let html: &'static str = r#"<html><body>
        <div class="ch" data-id="ch-1"><span class="title">Chapter 1</span></div>
        <div class="ch" data-id="ch-2"><span class="title">Chapter 2</span></div>
    </body></html>"#;

    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let chapter_ep = ValidatedEndpoint {
        route: "/manga/$manga_id$/chapters".into(),
        fields: vec![
            self_attr_field("id", "data-id"),
            text_field("title", ".title"),
        ],
        container: ".ch".into(),
        ..list_endpoint("/manga/$manga_id$/chapters", ".ch")
    };

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            chapter_list: Some(chapter_ep),
            ..Default::default()
        },
    );

    let result = src
        .get_chapter_list("manga-1", 1, None, None)
        .await
        .unwrap();

    assert_eq!(result.chapters.len(), 2);
    assert_eq!(result.chapters[0].id, "ch-1");
    assert_eq!(result.chapters[1].id, "ch-2");
    assert!(!result.has_next_page);
}

#[tokio::test]
async fn get_pages_extracts_page_urls_from_html() {
    let html: &'static str = r#"<html><body>
        <div class="page" data-url="https://cdn.example.com/p1.jpg"></div>
        <div class="page" data-url="https://cdn.example.com/p2.jpg"></div>
    </body></html>"#;

    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let pages_ep = ValidatedEndpoint {
        route: "/manga/$manga_id$/chapter/$chapter_id$".into(),
        fields: vec![ValidatedField {
            name: "url".to_string(),
            source: FieldSource::Blueprint(Expr::Attr {
                target: Box::new(Expr::SelfRef),
                name: "data-url".to_string(),
            }),
            optional: false,
        }],
        container: ".page".into(),
        ..list_endpoint("/manga/$manga_id$/chapter/$chapter_id$", ".page")
    };

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            pages: Some(pages_ep),
            ..Default::default()
        },
    );

    let result = src.get_pages("manga-1", "ch-1").await.unwrap();

    assert_eq!(result.pages.len(), 2);
    assert_eq!(result.pages[0].url, "https://cdn.example.com/p1.jpg");
    assert_eq!(result.pages[0].index, 0);
    assert_eq!(result.pages[1].url, "https://cdn.example.com/p2.jpg");
    assert_eq!(result.pages[1].index, 1);
}

#[tokio::test]
async fn get_filter_list_maps_all_kinds_and_options() {
    let src = yaml_source(
        "http://127.0.0.1:1",
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: "http://127.0.0.1:1".into(),
            language: "en".into(),
            filters: vec![
                FilterEntry {
                    id: "genre".into(),
                    name: "Genre".into(),
                    kind: FilterKind::Select,
                    options: vec![
                        SchemaFilterOption {
                            name: "Action".into(),
                            value: "action".into(),
                            nsfw: false,
                        },
                        SchemaFilterOption {
                            name: "Romance".into(),
                            value: "romance".into(),
                            nsfw: false,
                        },
                    ],
                    default: None,
                    semantic: None,
                    name_i18n: None,
                    options_ref: None,
                    min: None,
                    max: None,
                    step: None,
                },
                FilterEntry {
                    id: "author".into(),
                    name: "Author".into(),
                    kind: FilterKind::TextInput,
                    options: vec![],
                    default: None,
                    semantic: Some(FilterSemantic::Author),
                    name_i18n: None,
                    options_ref: None,
                    min: None,
                    max: None,
                    step: None,
                },
                FilterEntry {
                    id: "completed".into(),
                    name: "Completed".into(),
                    kind: FilterKind::Checkbox,
                    options: vec![],
                    default: Some(kani_yaml::yaml::schema::FilterDefault::Bool(false)),
                    semantic: None,
                    name_i18n: None,
                    options_ref: None,
                    min: None,
                    max: None,
                    step: None,
                },
                FilterEntry {
                    id: "year_range".into(),
                    name: "Year".into(),
                    kind: FilterKind::IntRange,
                    options: vec![],
                    default: None,
                    semantic: None,
                    name_i18n: None,
                    options_ref: None,
                    min: Some(2000.0),
                    max: Some(2025.0),
                    step: None,
                },
            ],
            ..Default::default()
        },
    );

    let filter_list = src.get_filter_list().await.unwrap();

    assert_eq!(filter_list.filters.len(), 4);

    let genre = &filter_list.filters[0];
    assert_eq!(genre.id, "genre");
    assert_eq!(genre.name, "Genre");
    assert!(matches!(
        genre.tag,
        kani_core::wasm::kani::extension::types::FilterTypeTag::Select
    ));
    assert_eq!(genre.options.len(), 2);
    assert_eq!(genre.options[0].filter_name, "genre");
    assert_eq!(genre.options[0].name, "Action");
    assert_eq!(genre.options[0].value, "action");

    let author = &filter_list.filters[1];
    assert_eq!(author.id, "author");
    assert!(matches!(
        author.tag,
        kani_core::wasm::kani::extension::types::FilterTypeTag::TextInput
    ));
    assert!(matches!(
        author.semantic,
        Some(kani_core::wasm::kani::extension::types::FilterSemantic::Author)
    ));

    let completed = &filter_list.filters[2];
    assert_eq!(completed.id, "completed");
    assert!(matches!(
        completed.tag,
        kani_core::wasm::kani::extension::types::FilterTypeTag::Checkbox
    ));
    assert!(matches!(
        completed.default_value,
        Some(kani_core::wasm::kani::extension::types::FilterState::Checkbox(false))
    ));

    let year = &filter_list.filters[3];
    assert_eq!(year.id, "year_range");
    assert!(matches!(
        year.tag,
        kani_core::wasm::kani::extension::types::FilterTypeTag::TextInput
    ));
}

#[tokio::test]
async fn get_fetched_option_sets_lists_fetch_configured_filters() {
    use kani_yaml::yaml::schema::{FetchedOptionsDef, OptionSetDef, ResponseType};
    use std::collections::BTreeMap;

    let mut option_sets = BTreeMap::new();
    option_sets.insert(
        "genres".to_string(),
        OptionSetDef::Fetched {
            options_fetched_by: FetchedOptionsDef {
                route: "/genres".into(),
                response_type: ResponseType::Html,
                container: Some(".genre".into()),
                fields: BTreeMap::from([
                    ("name".to_string(), ".name".to_string()),
                    ("value".to_string(), "data-id".to_string()),
                ]),
                nsfw_field: None,
                cache: None,
            },
        },
    );

    let src = yaml_source(
        "http://127.0.0.1:1",
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: "http://127.0.0.1:1".into(),
            language: "en".into(),
            filters: vec![
                FilterEntry {
                    id: "genre".into(),
                    name: "Genre".into(),
                    kind: FilterKind::Select,
                    options: vec![],
                    default: None,
                    semantic: None,
                    name_i18n: None,
                    options_ref: Some("genres".into()),
                    min: None,
                    max: None,
                    step: None,
                },
                FilterEntry {
                    id: "author".into(),
                    name: "Author".into(),
                    kind: FilterKind::TextInput,
                    options: vec![],
                    default: None,
                    semantic: Some(FilterSemantic::Author),
                    name_i18n: None,
                    options_ref: None,
                    min: None,
                    max: None,
                    step: None,
                },
            ],
            option_sets,
            ..Default::default()
        },
    );

    let raw = src.get_fetched_option_sets().await.unwrap();
    let parsed: Vec<kani_shared::filter_fetch::FilterFetchDef> =
        serde_json::from_str(&raw).unwrap();

    assert_eq!(
        parsed.len(),
        1,
        "only the filter with a Fetched options_ref should be listed"
    );
    let entry = &parsed[0];
    assert_eq!(entry.filter_id, "genre");
    assert_eq!(entry.option_set_name, "genres");
    assert_eq!(entry.route, "/genres");
    assert_eq!(entry.response_type, "html");
    assert_eq!(entry.container.as_deref(), Some(".genre"));
    assert_eq!(entry.cache_ttl, 300, "default TTL when no cache block set");
}

#[tokio::test]
async fn get_preferences_maps_all_kinds() {
    let src = yaml_source(
        "http://127.0.0.1:1",
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: "http://127.0.0.1:1".into(),
            language: "en".into(),
            preferences: vec![
                PreferenceEntry {
                    key: "enable_nsfw".into(),
                    label: "Show NSFW".into(),
                    kind: PreferenceKind::Toggle,
                    options: vec![],
                    default: "false".into(),
                    description: Some("Enable NSFW content".into()),
                    secret: false,
                    options_ref: None,
                },
                PreferenceEntry {
                    key: "quality".into(),
                    label: "Image Quality".into(),
                    kind: PreferenceKind::Select,
                    options: vec![
                        kani_yaml::yaml::schema::PrefOption {
                            name: "High".into(),
                            value: "high".into(),
                        },
                        kani_yaml::yaml::schema::PrefOption {
                            name: "Low".into(),
                            value: "low".into(),
                        },
                    ],
                    default: "high".into(),
                    description: None,
                    secret: false,
                    options_ref: None,
                },
            ],
            ..Default::default()
        },
    );

    let prefs = src.get_preferences().await.unwrap();

    assert_eq!(prefs.len(), 2);

    let toggle = &prefs[0];
    assert_eq!(toggle.key, "enable_nsfw");
    assert_eq!(toggle.label, "Show NSFW");
    assert!(matches!(
        toggle.kind,
        kani_core::wasm::kani::extension::types::PrefKind::Toggle
    ));
    assert_eq!(toggle.default, "false");
    assert_eq!(toggle.description, Some("Enable NSFW content".into()));

    let select = &prefs[1];
    assert_eq!(select.key, "quality");
    assert!(matches!(
        select.kind,
        kani_core::wasm::kani::extension::types::PrefKind::Select
    ));
    assert_eq!(select.options.len(), 2);
    assert_eq!(select.options[0], ("High".to_string(), "high".to_string()));
}

#[tokio::test]
async fn scan_yaml_registers_in_db_and_loads_into_registry() {
    let html: &'static str = r#"<html><body>
        <div class="item" data-id="m-1"><span class="title">Scan Test</span></div>
    </body></html>"#;
    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let dir = tempfile::tempdir().unwrap();
    let yaml_content = format!(
        r#"id: scan-test-source
name: scan-test-source
version: "1.0.0"
base_url: "{base_url}"
language: en
requires_capabilities:
  - unrestricted_http
endpoints:
  popular:
    route: /popular
    container: ".item"
    fields:
      id: 'self.attr("data-id")'
      title: 'self.first(".title").text()'
"#
    );
    std::fs::write(dir.path().join("scan-test-source.yaml"), &yaml_content).unwrap();

    let svc = test_service().await;
    svc.scan_and_load_yaml_dir_for_test(dir.path())
        .await
        .unwrap();

    use sqlx::Row as _;
    let row =
        sqlx::query("SELECT enabled, load_error FROM sources WHERE name = 'scan-test-source'")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    let enabled: i64 = row.try_get("enabled").unwrap();
    let load_error: Option<String> = row.try_get("load_error").unwrap();
    assert_eq!(enabled, 1, "source should be enabled after successful scan");
    assert!(load_error.is_none(), "load_error should be NULL on success");

    let source_id: i64 =
        sqlx::query_scalar("SELECT id FROM sources WHERE name = 'scan-test-source'")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert!(
        svc.sources.contains_key(source_id),
        "source should be in registry after scan+load"
    );

    let backend = svc.sources.get_backend(source_id).unwrap();
    let result = backend.get_popular_manga(1, 20, &[]).await.unwrap();
    assert_eq!(result.manga.len(), 1);
    assert_eq!(result.manga[0].id, "m-1");
    assert_eq!(result.manga[0].title, "Scan Test");
}

#[tokio::test]
async fn scan_yaml_with_bad_capability_sets_load_error_and_disables() {
    let dir = tempfile::tempdir().unwrap();
    let yaml_content = r#"id: bad-cap-source
name: bad-cap-source
version: "1.0.0"
base_url: "https://example.com"
language: en
requires_capabilities:
  - nonexistent_capability
endpoints: {}
"#;
    std::fs::write(dir.path().join("bad-cap-source.yaml"), yaml_content).unwrap();

    let svc = test_service().await;
    svc.scan_and_load_yaml_dir_for_test(dir.path())
        .await
        .unwrap();

    use sqlx::Row as _;
    let row = sqlx::query("SELECT enabled, load_error FROM sources WHERE name = 'bad-cap-source'")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let enabled: i64 = row.try_get("enabled").unwrap();
    let load_error: Option<String> = row.try_get("load_error").unwrap();
    assert_eq!(enabled, 0, "source with bad capability should be disabled");
    assert!(
        load_error.is_some(),
        "load_error should be set on capability mismatch"
    );
    assert!(
        load_error.unwrap().contains("nonexistent_capability"),
        "error message should name the missing capability"
    );

    let source_id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM sources WHERE name = 'bad-cap-source'")
            .fetch_optional(&svc.db)
            .await
            .unwrap();
    if let Some(id) = source_id {
        assert!(
            !svc.sources.contains_key(id),
            "source with bad capability must not be in registry"
        );
    }
}

#[tokio::test]
async fn yaml_supersedes_wasm_when_both_files_present_in_dir() {
    let html: &'static str = r#"<html><body>
        <div class="item" data-id="y-1"><span class="title">Yaml Wins</span></div>
    </body></html>"#;
    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let dir = tempfile::tempdir().unwrap();
    let yaml_content = format!(
        r#"id: supersede-src
name: supersede-src
version: "1.0.0"
base_url: "{base_url}"
language: en
requires_capabilities:
  - unrestricted_http
endpoints:
  popular:
    route: /popular
    container: ".item"
    fields:
      id: 'self.attr("data-id")'
      title: 'self.first(".title").text()'
"#
    );
    std::fs::write(dir.path().join("supersede-src.yaml"), &yaml_content).unwrap();

    let svc = test_service().await;
    svc.scan_and_load_yaml_dir_for_test(dir.path())
        .await
        .unwrap();

    let source_id: i64 = sqlx::query_scalar("SELECT id FROM sources WHERE name = 'supersede-src'")
        .fetch_one(&svc.db)
        .await
        .unwrap();

    std::fs::write(dir.path().join("supersede-src.wasm"), b"not-a-real-wasm").unwrap();

    svc.load_yaml_sources_from_dir_for_test(dir.path())
        .await
        .unwrap();

    let backend = svc.sources.get_backend(source_id).unwrap();
    assert!(
        backend.is_yaml(),
        "YAML backend must be selected when both .yaml and .wasm exist for the same source"
    );

    let result = backend.get_popular_manga(1, 20, &[]).await.unwrap();
    assert_eq!(result.manga[0].title, "Yaml Wins");
}

#[tokio::test]
async fn reload_source_rereads_a_yaml_extension_from_disk() {
    let html_v1: &'static str = r#"<html><body>
        <div class="item" data-id="m-1"><span class="title">Before Reload</span></div>
    </body></html>"#;
    let port = start_html_server(html_v1).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let svc = test_service().await;
    let storage_path = svc.settings.read().await.wasm_storage_path.clone();

    let yaml_v1 = format!(
        r#"id: reload-test-source
name: reload-test-source
version: "1.0.0"
base_url: "{base_url}"
language: en
requires_capabilities:
  - unrestricted_http
endpoints:
  popular:
    route: /popular
    container: ".item"
    fields:
      id: 'self.attr("data-id")'
      title: 'self.first(".title").text()'
"#
    );
    std::fs::write(storage_path.join("reload-test-source.yaml"), &yaml_v1).unwrap();

    let source_id = common::insert_source(&svc.db, "reload-test-source").await;
    svc.sources.insert(
        source_id,
        SourceBackend::Yaml(Box::new(YamlSource::new(
            Arc::new(kani_yaml::parse_and_validate(&yaml_v1, std::path::Path::new("x")).unwrap()),
            svc.smart_client.clone(),
            svc.ext_cache.clone(),
            "reload-test-source:".to_string(),
            HashMap::new(),
            false,
        ))),
    );

    let yaml_v2 = yaml_v1.replace("1.0.0", "2.0.0");
    std::fs::write(storage_path.join("reload-test-source.yaml"), &yaml_v2).unwrap();

    svc.reload_source(source_id).await.unwrap();

    let version: String = sqlx::query_scalar("SELECT version FROM sources WHERE id = ?")
        .bind(source_id)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(version, "2.0.0", "reload must persist the re-read version");

    let backend = svc.sources.get_backend(source_id).unwrap();
    assert!(
        backend.is_yaml(),
        "reload must not turn a YAML source into a WASM one"
    );
    let result = backend.get_popular_manga(1, 20, &[]).await.unwrap();
    assert_eq!(
        result.manga[0].title, "Before Reload",
        "swapped-in backend must serve live requests, not stale cached data"
    );
}

#[tokio::test]
async fn app_service_manga_and_chapter_endpoints_accept_base64_composite_ids_verbatim() {
    let html: &'static str = r#"<html><body>
        <div class="item" data-hid="h1" data-slug="some-title-slug"><span class="title">Some Title</span></div>
        <div class="chapter" data-chid="c1" data-chslug="chapter-1-slug" data-number="1"></div>
        <div class="page" data-url="http://example.com/p1.jpg"></div>
    </body></html>"#;
    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let yaml = format!(
        r#"id: repro-source
name: repro-source
version: "1.0.0"
base_url: "{base_url}"
language: en
requires_capabilities:
  - unrestricted_http
get_url: "/title/$manga.slug$"
id_encoding:
  manga:
    fields: [hid, slug]
    delimiter: "|"
    encoding: base64_url
  chapter:
    fields: [id, slug]
    delimiter: "|"
    encoding: base64_url
endpoints:
  popular:
    route: /popular
    container: ".item"
    fields:
      id:
        hid: 'self.attr("data-hid")'
        slug: 'self.attr("data-slug")'
      title: 'self.first(".title").text()'
  manga_details:
    route: /title/$manga.slug$
    container: ":root"
    fields:
      id: '"$manga_id$"'
      title: 'self.first(".title").text()'
      status: '"unknown"'
  chapter_list:
    route: /title/$manga.slug$
    container: ".chapter"
    fields:
      id:
        id: 'self.attr("data-chid")'
        slug: 'self.attr("data-chslug")'
      number: "1.0"
      language: '"en"'
  pages:
    route: /title/$manga.slug$/$chapter.slug$
    container: ".page"
    fields:
      index: "index()"
      url: 'self.attr("data-url")'
"#
    );

    let svc = test_service().await;
    let source_id = svc.install_yaml_source(yaml.as_bytes()).await.unwrap();

    let popular_json = svc.get_popular_manga(source_id, 1, 20, None).await.unwrap();
    let popular: serde_json::Value = serde_json::from_str(&popular_json).unwrap();
    // This is the exact id a client round-trips through every .../{manga_id} route —
    // the extension's own base64url composite, not something the host wraps further.
    let manga_id = popular["manga"][0]["id"].as_str().unwrap().to_string();

    let details = svc.get_manga_details(source_id, &manga_id).await;
    assert!(
        details.is_ok(),
        "get_manga_details must accept the id verbatim, got {:?}",
        details.err()
    );

    let url = svc.get_source_url(source_id, &manga_id).await;
    assert!(
        url.is_ok(),
        "get_source_url must accept the id verbatim, got {:?}",
        url.err()
    );

    let chapters_json = svc
        .get_chapter_list_paged(source_id, &manga_id, 1, 20, None)
        .await;
    assert!(
        chapters_json.is_ok(),
        "get_chapter_list_paged must accept the manga id verbatim, got {:?}",
        chapters_json.err()
    );
    let chapters: serde_json::Value = serde_json::from_str(&chapters_json.unwrap()).unwrap();
    let chapter_id = chapters["chapters"][0]["id"].as_str().unwrap().to_string();

    let pages = svc.get_pages(source_id, &manga_id, &chapter_id).await;
    assert!(
        pages.is_ok(),
        "get_pages must accept both composite ids verbatim, got {:?}",
        pages.err()
    );

    let saved = svc
        .save_to_library(source_id, &manga_id, false)
        .await
        .unwrap();
    let found = svc.check_in_library(source_id, &manga_id).await.unwrap();
    assert_eq!(
        found,
        Some(saved.0),
        "check_in_library must find a manga saved under the same composite id verbatim"
    );
}

#[tokio::test]
async fn browser_payload_endpoint_returns_clear_error() {
    use kani_yaml::yaml::schema::EndpointVia;

    let mut ep = list_endpoint("/popular", ".item");
    ep.via = Some(EndpointVia::BrowserPayload);

    let src = yaml_source(
        "http://127.0.0.1:1",
        ValidatedExtension {
            id: "bp-test".into(),
            name: "bp-test".into(),
            version: "1.0.0".into(),
            base_url: "http://127.0.0.1:1".into(),
            language: "en".into(),
            unrestricted_http: true,
            popular: Some(ValidatedPopular::Full(Box::new(ep))),
            ..Default::default()
        },
    );

    let result = src.get_popular_manga(1, 20, &[]).await;
    assert!(result.is_err(), "browser_payload must fail with error");
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("browser_payload") || err_str.contains("browser runtime"),
        "error should mention browser_payload: {err_str}"
    );
}

#[tokio::test]
async fn browser_payload_endpoint_missing_script_returns_clear_error() {
    use kani_yaml::yaml::schema::EndpointVia;

    let mut ep = list_endpoint("/popular", ".item");
    ep.via = Some(EndpointVia::BrowserPayload);
    ep.page_url = Some("https://example.com/manga/$manga_id$".into());
    ep.script_name = Some("undeclared_script".into());

    let src = yaml_source(
        "http://127.0.0.1:1",
        ValidatedExtension {
            id: "bp-test".into(),
            name: "bp-test".into(),
            version: "1.0.0".into(),
            base_url: "http://127.0.0.1:1".into(),
            language: "en".into(),
            unrestricted_http: true,
            manga_details: Some(ep),
            ..Default::default()
        },
    );

    let result = src.get_manga_details("manga-1").await;
    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("undeclared_script"),
        "error should name the missing script: {err_str}"
    );
}

#[tokio::test]
async fn installing_a_browser_source_without_a_solver_is_refused_with_guidance() {
    let yaml_content = br#"
id: install-browser-source
name: install-browser-source
version: "1.0.0"
base_url: "https://example.com"
language: en
requires_capabilities:
  - browser_payload
endpoints:
  popular:
    route: /popular
    container: ".item"
    fields:
      id: 'self.attr("data-id")'
      title: 'self.first(".title").text()'
"#;

    let svc = test_service().await;
    let error = svc
        .install_yaml_source(yaml_content)
        .await
        .expect_err("a browser source needs a capture-capable solver");
    let message = error.to_string();

    assert!(
        message.contains("capture scripts"),
        "the refusal names what is missing, got: {message}"
    );
    assert!(
        message.contains("Settings > Advanced"),
        "the refusal names where to fix it, got: {message}"
    );
}

#[tokio::test]
async fn the_startup_scan_does_not_disable_a_browser_source_without_a_solver() {
    let dir = tempfile::tempdir().unwrap();
    let yaml_content = r#"
id: scan-browser-source
name: scan-browser-source
version: "1.0.0"
base_url: "https://example.com"
language: en
requires_capabilities:
  - browser_payload
endpoints:
  popular:
    route: /popular
    container: ".item"
    fields:
      id: 'self.attr("data-id")'
      title: 'self.first(".title").text()'
"#;
    std::fs::write(dir.path().join("scan-browser-source.yaml"), yaml_content).unwrap();

    let svc = test_service().await;
    svc.scan_and_load_yaml_dir_for_test(dir.path())
        .await
        .unwrap();

    use sqlx::Row as _;
    let row =
        sqlx::query("SELECT enabled, load_error FROM sources WHERE name = 'scan-browser-source'")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    let enabled: i64 = row.try_get("enabled").unwrap();
    let load_error: Option<String> = row.try_get("load_error").unwrap();

    assert_eq!(
        enabled, 1,
        "solver reachability is dynamic; a source must not be disabled at boot because \
         the solver container happened to start second"
    );
    assert!(
        load_error.is_none(),
        "load_error records static invalidity, not solver state, got: {load_error:?}"
    );
}

#[tokio::test]
async fn browser_payload_endpoint_reaches_capture_page_payload() {
    use kani_yaml::yaml::schema::EndpointVia;

    let mut ep = list_endpoint("/popular", ".item");
    ep.via = Some(EndpointVia::BrowserPayload);
    ep.page_url = Some("https://example.com/manga/$manga_id$".into());
    ep.script_name = Some("fetch_manga".into());

    let mut browser_scripts = std::collections::BTreeMap::new();
    browser_scripts.insert("fetch_manga".to_string(), "passPayload('{}');".to_string());

    let src = yaml_source(
        "http://127.0.0.1:1",
        ValidatedExtension {
            id: "bp-test".into(),
            name: "bp-test".into(),
            version: "1.0.0".into(),
            base_url: "http://127.0.0.1:1".into(),
            language: "en".into(),
            unrestricted_http: true,
            manga_details: Some(ep),
            browser_scripts,
            ..Default::default()
        },
    );

    let result = src.get_manga_details("manga-1").await;
    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("solver_not_configured"),
        "a browser endpoint with no solver must say so, got: {err_str}"
    );
    assert!(
        err_str.contains("Settings > Advanced"),
        "the error must name where to fix it, got: {err_str}"
    );
}

#[tokio::test]
async fn browser_payload_restricted_host_rejected_before_dispatch() {
    use kani_yaml::yaml::schema::EndpointVia;

    // A restricted source (unrestricted_http = false) must not be able to point
    // the browser at an arbitrary host. The AllowedHost check fires before any V8
    // dispatch, so the error is host-specific rather than a browser-runtime error.
    let mut ep = list_endpoint("/popular", ".item");
    ep.via = Some(EndpointVia::BrowserPayload);
    ep.page_url = Some("https://evil.example.com/manga/$manga_id$".into());
    ep.script_name = Some("fetch_manga".into());

    let mut browser_scripts = std::collections::BTreeMap::new();
    browser_scripts.insert("fetch_manga".to_string(), "passPayload('{}');".to_string());

    let src = yaml_source(
        "http://127.0.0.1:1",
        ValidatedExtension {
            id: "bp-test".into(),
            name: "bp-test".into(),
            version: "1.0.0".into(),
            base_url: "http://127.0.0.1:1".into(),
            language: "en".into(),
            unrestricted_http: false,
            manga_details: Some(ep),
            browser_scripts,
            ..Default::default()
        },
    );

    let result = src.get_manga_details("manga-1").await;
    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("blocked")
            || err_str.contains("only contact")
            || err_str.contains("evil.example.com"),
        "restricted source should reject off-host browser target: {err_str}"
    );
}

#[tokio::test]
async fn browser_payload_rejected_when_browser_disabled_for_source() {
    use kani_yaml::yaml::schema::EndpointVia;

    let mut ep = list_endpoint("/popular", ".item");
    ep.via = Some(EndpointVia::BrowserPayload);
    ep.page_url = Some("https://example.com/manga/$manga_id$".into());
    ep.script_name = Some("fetch_manga".into());

    let mut browser_scripts = std::collections::BTreeMap::new();
    browser_scripts.insert("fetch_manga".to_string(), "passPayload('{}');".to_string());

    let ext = ValidatedExtension {
        id: "bp-test".into(),
        name: "bp-test".into(),
        version: "1.0.0".into(),
        base_url: "http://127.0.0.1:1".into(),
        language: "en".into(),
        unrestricted_http: true,
        manga_details: Some(ep),
        browser_scripts,
        ..Default::default()
    };
    let src = yaml_source_with_browser(ext, false);

    let result = src.get_manga_details("manga-1").await;
    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("disabled"),
        "source with browser disabled should reject before dispatch: {err_str}"
    );
}

#[tokio::test]
async fn refresh_auth_retries_and_succeeds() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let popular_hits = Arc::new(AtomicU32::new(0));
    let login_hits = Arc::new(AtomicU32::new(0));
    let popular_hits_srv = Arc::clone(&popular_hits);
    let login_hits_srv = Arc::clone(&login_hits);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let ph = Arc::clone(&popular_hits_srv);
            let lh = Arc::clone(&login_hits_srv);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req_str = std::str::from_utf8(&buf[..n]).unwrap_or("");
                let (status, body): (u16, &str) = if req_str.contains("GET /login") {
                    lh.fetch_add(1, Ordering::Relaxed);
                    (200, r#"<html><body></body></html>"#)
                } else {
                    let prev = ph.fetch_add(1, Ordering::Relaxed);
                    if prev == 0 {
                        (401, "")
                    } else {
                        (
                            200,
                            r#"<html><body><div class="item" data-id="r-1"><span class="title">Retried</span></div></body></html>"#,
                        )
                    }
                };
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    let base_url = format!("http://127.0.0.1:{port}");

    let mut on_status = std::collections::BTreeMap::new();
    on_status.insert("401".to_string(), r#"refresh_auth("search")"#.to_string());

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "auth-test".into(),
            name: "auth-test".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            popular: Some(ValidatedPopular::Full(Box::new(list_endpoint(
                "/popular", ".item",
            )))),
            search: Some(list_endpoint("/login", ".item")),
            on_status,
            ..Default::default()
        },
    );

    let result = src.get_popular_manga(1, 20, &[]).await.unwrap();
    assert_eq!(result.manga.len(), 1, "should succeed on retry");
    assert_eq!(result.manga[0].id, "r-1");
    assert_eq!(result.manga[0].title, "Retried");
    assert_eq!(
        popular_hits.load(Ordering::Relaxed),
        2,
        "/popular must be called exactly twice: once returning 401, once returning 200"
    );
    assert_eq!(
        login_hits.load(Ordering::Relaxed),
        1,
        "/login (auth endpoint) must be called exactly once by the refresh_auth dispatch"
    );
}

#[tokio::test]
async fn yaml_hot_swap_in_flight_call_completes_with_old_config() {
    let html_old: &'static str = r#"<html><body>
        <div class="item" data-id="old-1"><span class="title">Old Config</span></div>
    </body></html>"#;
    let html_new: &'static str = r#"<html><body>
        <div class="item" data-id="new-1"><span class="title">New Config</span></div>
    </body></html>"#;

    let port_old = start_html_server(html_old).await;
    let port_new = start_html_server(html_new).await;
    let base_old = format!("http://127.0.0.1:{port_old}");
    let base_new = format!("http://127.0.0.1:{port_new}");

    let registry = SourceRegistry::new();

    registry.insert(
        1,
        SourceBackend::Yaml(Box::new(yaml_source(
            &base_old,
            ValidatedExtension {
                id: "swap-src".into(),
                name: "swap-src".into(),
                version: "1.0.0".into(),
                base_url: base_old.clone(),
                language: "en".into(),
                unrestricted_http: true,
                popular: Some(ValidatedPopular::Full(Box::new(list_endpoint(
                    "/popular", ".item",
                )))),
                ..Default::default()
            },
        ))),
    );

    let old_backend = registry.get_backend(1).unwrap();

    registry
        .hot_swap(
            1,
            SourceBackend::Yaml(Box::new(yaml_source(
                &base_new,
                ValidatedExtension {
                    id: "swap-src".into(),
                    name: "swap-src".into(),
                    version: "2.0.0".into(),
                    base_url: base_new.clone(),
                    language: "en".into(),
                    unrestricted_http: true,
                    popular: Some(ValidatedPopular::Full(Box::new(list_endpoint(
                        "/popular", ".item",
                    )))),
                    ..Default::default()
                },
            ))),
        )
        .await;

    // In-flight call completes with old config.
    let old_result = old_backend.get_popular_manga(1, 20, &[]).await.unwrap();
    assert_eq!(
        old_result.manga[0].id, "old-1",
        "in-flight call must use pre-swap config"
    );

    // New call through the registry resolves to the swapped config.
    let new_result = registry
        .get_backend(1)
        .unwrap()
        .get_popular_manga(1, 20, &[])
        .await
        .unwrap();
    assert_eq!(
        new_result.manga[0].id, "new-1",
        "post-swap call must use new config"
    );
}

#[tokio::test]
async fn a_disabled_yaml_source_can_be_re_enabled() {
    let html: &'static str = r#"<html><body>
        <div class="item" data-id="m-1"><span class="title">Re-enable</span></div>
    </body></html>"#;
    let port = start_html_server(html).await;
    let base_url = format!("http://127.0.0.1:{port}");

    let dir = tempfile::tempdir().unwrap();
    let yaml_content = format!(
        r#"id: reenable-source
name: reenable-source
version: "1.0.0"
base_url: "{base_url}"
language: en
requires_capabilities:
  - unrestricted_http
endpoints:
  search:
    route: /search
    container: ".item"
    fields:
      id: 'self.attr("data-id")'
      title: 'self.first(".title").text()'
"#
    );
    std::fs::write(dir.path().join("reenable-source.yaml"), &yaml_content).unwrap();

    let svc = test_service().await;
    {
        let mut s = svc.settings.write().await;
        s.wasm_storage_path = dir.path().to_path_buf();
    }
    svc.scan_and_load_yaml_dir_for_test(dir.path())
        .await
        .unwrap();

    use sqlx::Row as _;
    let id: i64 = sqlx::query("SELECT id FROM sources WHERE name = 'reenable-source'")
        .fetch_one(&svc.db)
        .await
        .unwrap()
        .try_get("id")
        .unwrap();

    svc.toggle_source_enabled(id, false).await.unwrap();
    svc.toggle_source_enabled(id, true)
        .await
        .expect("re-enabling a YAML source must not fail reading a nonexistent .wasm");

    let result = svc.search_manga(id, "anything", 1, 20, None).await;
    assert!(
        result.is_ok(),
        "a re-enabled YAML source must serve requests, got {result:?}"
    );
}

async fn start_status_server(status_line: &'static str, extra_headers: &'static str) -> u16 {
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
                let body = "rate limited";
                let response = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: text/html\r\n{extra_headers}\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len(),
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    port
}

fn extension_error(err: kani_core::error::Error) -> kani_shared::extension::ExtensionError {
    match err {
        kani_core::error::Error::Extension(e) => e,
        other => panic!("expected Error::Extension, got {other:?}"),
    }
}

#[tokio::test]
async fn yaml_source_429_classifies_as_rate_limited_with_retry_after() {
    use kani_shared::extension::ExtensionErrorKind;

    let port = start_status_server("429 Too Many Requests", "Retry-After: 120\r\n").await;
    let base_url = format!("http://127.0.0.1:{port}");

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "rl-source".into(),
            name: "RL Source".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            popular: Some(ValidatedPopular::Full(Box::new(list_endpoint(
                "/popular", ".item",
            )))),
            ..Default::default()
        },
    );

    let err = extension_error(src.get_popular_manga(1, 20, &[]).await.unwrap_err());
    assert_eq!(
        err.kind,
        ExtensionErrorKind::RateLimited,
        "a 429 must classify as RateLimited, not Parse"
    );
    assert_eq!(
        err.retry_after_secs,
        Some(120),
        "the server's Retry-After must survive to the typed error"
    );
}

#[tokio::test]
async fn yaml_source_503_classifies_as_retryable_network() {
    use kani_shared::extension::ExtensionErrorKind;

    let port = start_status_server("503 Service Unavailable", "").await;
    let base_url = format!("http://127.0.0.1:{port}");

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "svc-source".into(),
            name: "Svc Source".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            popular: Some(ValidatedPopular::Full(Box::new(list_endpoint(
                "/popular", ".item",
            )))),
            ..Default::default()
        },
    );

    let err = extension_error(src.get_popular_manga(1, 20, &[]).await.unwrap_err());
    assert_eq!(
        err.kind,
        ExtensionErrorKind::Network,
        "a 5xx must classify as retryable Network, not Parse"
    );
}

#[tokio::test]
async fn yaml_source_404_is_not_surfaced_as_a_typed_http_error() {
    let port = start_status_server("404 Not Found", "").await;
    let base_url = format!("http://127.0.0.1:{port}");

    let src = yaml_source(
        &base_url,
        ValidatedExtension {
            id: "nf-source".into(),
            name: "NF Source".into(),
            version: "1.0.0".into(),
            base_url: base_url.clone(),
            language: "en".into(),
            unrestricted_http: true,
            popular: Some(ValidatedPopular::Full(Box::new(list_endpoint(
                "/popular", ".item",
            )))),
            ..Default::default()
        },
    );

    let result = src
        .get_popular_manga(1, 20, &[])
        .await
        .expect("a 404 must not surface as a typed HTTP error");
    assert!(result.manga.is_empty());
}

#[tokio::test]
async fn a_source_supplied_id_cannot_rewrite_the_request_path() {
    use kani_shared_test::origin::{Response, TestOrigin};

    let origin = TestOrigin::start().await;
    origin.set("/manga/..%2Fadmin", Response::html("<html></html>"));

    let details_ep = ValidatedEndpoint {
        route: "/manga/$manga_id$".into(),
        fields: vec![self_attr_field("id", "data-id"), text_field("title", "h1")],
        container: ".manga".into(),
        ..list_endpoint("/manga/$manga_id$", ".manga")
    };

    let src = yaml_source(
        &origin.base(),
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: origin.base(),
            language: "en".into(),
            unrestricted_http: true,
            manga_details: Some(details_ep),
            ..Default::default()
        },
    );

    let _ = src.get_manga_details("../admin").await;

    assert_eq!(
        origin.hits("/manga/..%2Fadmin"),
        1,
        "the id must reach the origin percent-encoded, not as a path traversal"
    );
}

#[tokio::test]
async fn an_unresolved_route_placeholder_is_an_error_not_a_literal() {
    use kani_shared_test::origin::{Response, TestOrigin};

    let origin = TestOrigin::start().await;
    origin.set("/list", Response::html("<html></html>"));

    let ep = ValidatedEndpoint {
        route: "/list/$missing$".into(),
        ..list_endpoint("/list/$missing$", ".item")
    };

    let src = yaml_source(
        &origin.base(),
        ValidatedExtension {
            id: "fixture-source".into(),
            name: "Fixture Source".into(),
            version: "1.0.0".into(),
            base_url: origin.base(),
            language: "en".into(),
            unrestricted_http: true,
            popular: Some(ValidatedPopular::Full(Box::new(ep))),
            ..Default::default()
        },
    );

    let result = src.get_popular_manga(1, 20, &[]).await;
    assert!(
        result.is_err(),
        "an unresolved route placeholder must fail the call, not send a literal"
    );
    assert_eq!(
        origin.hits("/list"),
        0,
        "no request must be sent when a placeholder is unresolved"
    );
}

fn recoverable_yaml(version: &str) -> String {
    format!(
        r#"id: recover-me
name: recover-me
version: "{version}"
base_url: "https://example.com"
language: en
endpoints:
  popular:
    route: /popular
    container: ".item"
    fields:
      id: 'self.attr("data-id")'
      title: 'self.first(".title").text()'
"#
    )
}

async fn fail_source_writes(svc: &kani_app::service::AppService, event: &str) {
    sqlx::query(&format!(
        "CREATE TRIGGER inject_failure BEFORE {event} ON sources \
         BEGIN SELECT RAISE(ABORT, 'injected row failure'); END"
    ))
    .execute(&svc.db)
    .await
    .unwrap();
}

#[tokio::test]
async fn a_failed_row_update_restores_the_previous_artifact() {
    let svc = test_service().await;
    let storage = svc.settings.read().await.wasm_storage_path.clone();
    let v1 = recoverable_yaml("1.0.0");
    svc.install_yaml_source(v1.as_bytes()).await.unwrap();
    fail_source_writes(&svc, "UPDATE").await;

    let error = svc
        .install_yaml_source(recoverable_yaml("2.0.0").as_bytes())
        .await
        .expect_err("the injected trigger must fail the row update");

    assert!(
        error.to_string().contains("injected row failure"),
        "failed for the injected reason, got: {error}"
    );
    assert_eq!(
        tokio::fs::read_to_string(storage.join("recover-me.yaml"))
            .await
            .unwrap(),
        v1,
        "the artifact on disk must still match the row"
    );
}

#[tokio::test]
async fn a_failed_first_install_leaves_no_artifact() {
    let svc = test_service().await;
    let storage = svc.settings.read().await.wasm_storage_path.clone();
    fail_source_writes(&svc, "INSERT").await;

    let error = svc
        .install_yaml_source(recoverable_yaml("1.0.0").as_bytes())
        .await
        .expect_err("the injected trigger must fail the row insert");

    assert!(
        error.to_string().contains("injected row failure"),
        "failed for the injected reason, got: {error}"
    );
    assert!(
        !storage.join("recover-me.yaml").exists(),
        "no artifact may be left behind without a row"
    );
}

#[tokio::test]
async fn a_version_change_clears_the_extension_cache() {
    let svc = test_service().await;
    let sid = svc
        .install_yaml_source(recoverable_yaml("1.0.0").as_bytes())
        .await
        .unwrap();
    let ttl = std::time::Duration::from_secs(600);
    let seed = || async {
        for ns in ["recover-me:", "recover-me:auth"] {
            svc.ext_cache.put(ns, "k", b"v".to_vec(), ttl).await;
        }
        svc.ext_cache
            .put(&format!("fetched_opts:{sid}"), "k", b"v".to_vec(), ttl)
            .await;
    };
    let cached = || async {
        let mut present = Vec::new();
        for ns in [
            "recover-me:".to_string(),
            "recover-me:auth".to_string(),
            format!("fetched_opts:{sid}"),
        ] {
            if svc.ext_cache.get(&ns, "k").await.is_some() {
                present.push(ns);
            }
        }
        present
    };

    seed().await;
    svc.install_yaml_source(recoverable_yaml("1.0.0").as_bytes())
        .await
        .unwrap();
    assert_eq!(
        cached().await.len(),
        3,
        "a same-version reinstall keeps the cache"
    );

    svc.install_yaml_source(recoverable_yaml("2.0.0").as_bytes())
        .await
        .unwrap();
    assert!(
        cached().await.is_empty(),
        "a version change must clear every namespace, left: {:?}",
        cached().await
    );
}

#[tokio::test]
async fn a_version_change_found_at_startup_clears_the_extension_cache() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("recover-me.yaml");
    let svc = test_service().await;

    std::fs::write(&file, recoverable_yaml("1.0.0")).unwrap();
    svc.scan_and_load_yaml_dir_for_test(dir.path())
        .await
        .unwrap();
    let ttl = std::time::Duration::from_secs(600);
    svc.ext_cache
        .put("recover-me:auth", "k", b"v".to_vec(), ttl)
        .await;

    std::fs::write(&file, recoverable_yaml("2.0.0")).unwrap();
    svc.scan_and_load_yaml_dir_for_test(dir.path())
        .await
        .unwrap();

    assert!(
        svc.ext_cache.get("recover-me:auth", "k").await.is_none(),
        "a version bump on disk must clear the cache"
    );
}
