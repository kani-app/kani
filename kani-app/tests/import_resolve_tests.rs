#![allow(clippy::unwrap_used)]

//! Linking imported manga to ids their source accepts, driven against a real
//! `YamlSource` so the id under test is proved the same way production proves
//! it — by asking the source for the manga.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::service::import::resolve::Resolution;
use kani_app::source::{SourceBackend, YamlSource};
use kani_shared::ast::Expr;
use kani_shared_test::origin::{Response, TestOrigin};
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
    ValidatedTotalPages,
};
use kani_yaml::yaml::schema::{
    FilterDefault, FilterEntry, FilterKind, FilterMappingEntry, FilterOption, ResponseType,
};

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

fn attr_field(name: &str, attr: &str) -> ValidatedField {
    ValidatedField {
        name: name.to_string(),
        source: FieldSource::Blueprint(Expr::Attr {
            target: Box::new(Expr::SelfRef),
            name: attr.to_string(),
        }),
        optional: false,
    }
}

fn endpoint(route: &str, container: &str, fields: Vec<ValidatedField>) -> ValidatedEndpoint {
    ValidatedEndpoint {
        route: route.into(),
        method: "GET".into(),
        headers: vec![],
        queries: vec![],
        filter_mapping: vec![],
        filter_format: None,
        response_type: ResponseType::Html,
        container: container.into(),
        bindings: vec![],
        fields,
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

/// A source whose search declares a default the request is expected to carry.
fn sort_filter() -> FilterEntry {
    FilterEntry {
        id: "sort".into(),
        name: "Sort".into(),
        kind: FilterKind::Select,
        options: vec![
            FilterOption {
                name: "Best match".into(),
                value: "relevance".into(),
                nsfw: false,
            },
            FilterOption {
                name: "Newest".into(),
                value: "newest".into(),
                nsfw: false,
            },
        ],
        default: Some(FilterDefault::Option {
            name: "Best match".into(),
            value: "relevance".into(),
        }),
        semantic: None,
        name_i18n: None,
        options_ref: None,
        min: None,
        max: None,
        step: None,
    }
}

fn backend(origin: &TestOrigin) -> SourceBackend {
    let config = ValidatedExtension {
        id: "resolve".into(),
        name: "Resolve".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        manga_details: Some(endpoint(
            "/details?id=$manga_id$",
            ".manga",
            vec![attr_field("id", "data-id"), text_field("title", ".title")],
        )),
        search: Some(ValidatedEndpoint {
            filter_mapping: vec![("sort".into(), FilterMappingEntry::Simple("order".into()))],
            ..endpoint(
                "/search?q=$query$",
                ".item",
                vec![attr_field("id", "data-id"), text_field("title", ".title")],
            )
        }),
        filters: vec![sort_filter()],
        ..Default::default()
    };
    SourceBackend::Yaml(Box::new(YamlSource::new(
        Arc::new(config),
        kani_core::http::SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true),
        Arc::new(kani_core::cache::InMemoryCache::new()),
        "test:".into(),
        HashMap::new(),
        true,
    )))
}

const TITLE: &str = "Paper Cranes";
const IMPORTED_ID: &str = "/manga/mihon-shaped-id";
const REAL_ID: &str = "abc123";

fn details_page(id: &str, title: &str) -> String {
    format!(
        r#"<html><body><div class="manga" data-id="{id}"><h1 class="title">{title}</h1></div></body></html>"#
    )
}

/// A page the details blueprint cannot read a title out of, which is what a
/// source returns for an id it does not recognise.
fn unreadable_details() -> String {
    r#"<html><body><div class="manga"><p>Not found</p></div></body></html>"#.to_string()
}

fn search_page(id: &str, title: &str) -> String {
    format!(
        r#"<html><body><div class="item" data-id="{id}"><span class="title">{title}</span></div></body></html>"#
    )
}

async fn seed(
    svc: &kani_app::service::AppService,
    origin: &TestOrigin,
) -> (i64, kani_app::ids::MangaId) {
    let source_id = common::insert_source(&svc.db, "resolve-src").await;
    svc.sources.insert(source_id, backend(origin));
    let manga_id = common::insert_manga(&svc.db, source_id, IMPORTED_ID, TITLE).await;
    sqlx::query("INSERT INTO manga_import_links (manga_id, status) VALUES (?, 'pending')")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();
    (source_id, manga_id)
}

async fn link_status(
    svc: &kani_app::service::AppService,
    manga_id: kani_app::ids::MangaId,
) -> Option<String> {
    sqlx::query_scalar("SELECT status FROM manga_import_links WHERE manga_id = ?")
        .bind(manga_id.0)
        .fetch_optional(&svc.db)
        .await
        .unwrap()
}

async fn stored_id(
    svc: &kani_app::service::AppService,
    manga_id: kani_app::ids::MangaId,
) -> String {
    sqlx::query_scalar("SELECT source_manga_id FROM manga WHERE id = ?")
        .bind(manga_id.0)
        .fetch_one(&svc.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_imported_id_is_replaced_with_one_the_source_answers_to() {
    let origin = TestOrigin::start().await;
    origin.set("/details", Response::html(&unreadable_details()));
    origin.set("/search", Response::html(&search_page(REAL_ID, TITLE)));
    let svc = common::test_service().await;
    let (source_id, manga_id) = seed(&svc, &origin).await;

    // The replacement id is the only one the details page answers for, so the
    // relink can only succeed by proving it.
    origin.script(
        "/details",
        vec![
            Response::html(&unreadable_details()),
            Response::html(&details_page(REAL_ID, TITLE)),
        ],
    );

    let outcome = svc
        .resolve_imported_manga(source_id, manga_id, IMPORTED_ID, TITLE)
        .await
        .unwrap();

    assert_eq!(outcome, Resolution::Relinked(REAL_ID.to_string()));
    assert_eq!(stored_id(&svc, manga_id).await, REAL_ID);
    assert_eq!(link_status(&svc, manga_id).await, None);
}

#[tokio::test]
async fn a_candidate_the_source_will_not_open_is_never_stored() {
    let origin = TestOrigin::start().await;
    // Search offers a candidate, but no id ever produces a readable page.
    origin.set("/details", Response::html(&unreadable_details()));
    origin.set("/search", Response::html(&search_page(REAL_ID, TITLE)));
    let svc = common::test_service().await;
    let (source_id, manga_id) = seed(&svc, &origin).await;

    let outcome = svc
        .resolve_imported_manga(source_id, manga_id, IMPORTED_ID, TITLE)
        .await
        .unwrap();

    assert!(
        matches!(outcome, Resolution::Unresolved(_)),
        "an id that cannot be opened must not be stored: {outcome:?}"
    );
    assert_eq!(
        stored_id(&svc, manga_id).await,
        IMPORTED_ID,
        "an unresolved manga keeps the id it came in with"
    );
    assert_eq!(
        link_status(&svc, manga_id).await.as_deref(),
        Some("unlinked")
    );
}

#[tokio::test]
async fn a_source_outage_leaves_the_manga_queued_rather_than_unlinked() {
    let origin = TestOrigin::start().await;
    origin.set("/details", Response::status(503));
    origin.set("/search", Response::status(503));
    let svc = common::test_service().await;
    let (source_id, manga_id) = seed(&svc, &origin).await;

    let outcome = svc
        .resolve_imported_manga(source_id, manga_id, IMPORTED_ID, TITLE)
        .await
        .unwrap();

    assert!(
        matches!(outcome, Resolution::Deferred(_)),
        "a source that cannot answer says nothing about the id: {outcome:?}"
    );
    assert_eq!(
        link_status(&svc, manga_id).await.as_deref(),
        Some("pending"),
        "an outage must leave the manga in the queue, not give up on it"
    );
}

#[tokio::test]
async fn an_id_the_source_already_answers_to_is_left_alone() {
    let origin = TestOrigin::start().await;
    origin.set("/details", Response::html(&details_page(REAL_ID, TITLE)));
    let svc = common::test_service().await;
    let (source_id, manga_id) = seed(&svc, &origin).await;

    let outcome = svc
        .resolve_imported_manga(source_id, manga_id, IMPORTED_ID, TITLE)
        .await
        .unwrap();

    assert_eq!(outcome, Resolution::AlreadyValid);
    assert_eq!(stored_id(&svc, manga_id).await, IMPORTED_ID);
    assert_eq!(link_status(&svc, manga_id).await, None);
    assert_eq!(origin.hits("/search"), 0, "a working id needs no search");
}

const SUBTITLED: &str = "Bocchi the Rock! Side Story - Kikuri Hiroi's Heavy-Drinking Diary";

#[tokio::test]
async fn a_title_the_source_does_not_index_in_full_is_retried_shorter() {
    let origin = TestOrigin::start().await;
    // The full title finds nothing; the part before the subtitle finds the work.
    origin.script(
        "/search",
        vec![
            Response::html("<html><body></body></html>"),
            Response::html(&search_page(REAL_ID, SUBTITLED)),
        ],
    );
    origin.script(
        "/details",
        vec![
            Response::html(&unreadable_details()),
            Response::html(&details_page(REAL_ID, SUBTITLED)),
        ],
    );
    let svc = common::test_service().await;
    let source_id = common::insert_source(&svc.db, "resolve-src").await;
    svc.sources.insert(source_id, backend(&origin));
    let manga_id = common::insert_manga(&svc.db, source_id, IMPORTED_ID, SUBTITLED).await;
    sqlx::query("INSERT INTO manga_import_links (manga_id, status) VALUES (?, 'pending')")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let outcome = svc
        .resolve_imported_manga(source_id, manga_id, IMPORTED_ID, SUBTITLED)
        .await
        .unwrap();

    assert_eq!(outcome, Resolution::Relinked(REAL_ID.to_string()));
    assert_eq!(
        origin.hits("/search"),
        2,
        "the full title must be tried before anything shorter"
    );
    assert_eq!(
        origin
            .last_request("/search")
            .and_then(|r| r.query_param("q"))
            .as_deref(),
        Some("Bocchi%20the%20Rock%21%20Side%20Story"),
        "the second attempt drops the subtitle, not the title"
    );
    assert_eq!(stored_id(&svc, manga_id).await, REAL_ID);
    assert_eq!(link_status(&svc, manga_id).await, None);
}

#[tokio::test]
async fn a_title_no_rung_can_find_costs_at_most_the_capped_number_of_searches() {
    let origin = TestOrigin::start().await;
    origin.set("/search", Response::html("<html><body></body></html>"));
    origin.set("/details", Response::html(&unreadable_details()));
    let svc = common::test_service().await;
    let source_id = common::insert_source(&svc.db, "resolve-src").await;
    svc.sources.insert(source_id, backend(&origin));
    let manga_id = common::insert_manga(&svc.db, source_id, IMPORTED_ID, SUBTITLED).await;
    sqlx::query("INSERT INTO manga_import_links (manga_id, status) VALUES (?, 'pending')")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let outcome = svc
        .resolve_imported_manga(source_id, manga_id, IMPORTED_ID, SUBTITLED)
        .await
        .unwrap();

    assert!(matches!(outcome, Resolution::Unresolved(_)), "{outcome:?}");
    assert_eq!(
        origin.hits("/search"),
        3,
        "a browser source pays seconds per search, so the ladder is capped"
    );
    assert_eq!(
        link_status(&svc, manga_id).await.as_deref(),
        Some("unlinked")
    );
}

#[tokio::test]
async fn the_repair_search_carries_the_filters_the_source_declares_by_default() {
    let origin = TestOrigin::start().await;
    origin.set("/details", Response::html(&unreadable_details()));
    origin.set("/search", Response::html(&search_page(REAL_ID, TITLE)));
    let svc = common::test_service().await;
    let (source_id, manga_id) = seed(&svc, &origin).await;

    let _ = svc
        .resolve_imported_manga(source_id, manga_id, IMPORTED_ID, TITLE)
        .await
        .unwrap();

    // Without the declared default the site applies its own, which is what made
    // this search return something other than what the same search in the UI does.
    assert_eq!(
        origin
            .last_request("/search")
            .and_then(|r| r.query_param("order"))
            .as_deref(),
        Some("relevance"),
        "the repair search must send the same filters as every other search"
    );
}
