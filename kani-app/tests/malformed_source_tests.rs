#![allow(clippy::unwrap_used)]

//! Malformed and hostile source data against the interpreted
//! `YamlSource` (yaml-only: exercises the evaluator's container/row handling and
//! `unpack_*`). Drives a real popular endpoint against a `TestOrigin`.

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::source::{SourceBackend, YamlSource};
use kani_core::evaluator::EvalLimits;
use kani_shared::ast::Expr;
use kani_shared_test::origin::{Response, TestOrigin};
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
    ValidatedPopular, ValidatedTotalPages,
};
use kani_yaml::yaml::schema::ResponseType;

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

fn popular_endpoint() -> ValidatedEndpoint {
    ValidatedEndpoint {
        route: "/popular".into(),
        method: "GET".into(),
        headers: vec![],
        queries: vec![],
        filter_mapping: vec![],
        filter_format: None,
        response_type: ResponseType::Html,
        container: ".item".into(),
        bindings: vec![],
        fields: vec![attr_field("id", "data-id"), text_field("title", ".title")],
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

/// A YAML source with a `.item`/id+title popular endpoint pointed at `origin`,
/// with optional evaluator-limit override.
fn source(origin: &TestOrigin, limits: EvalLimits) -> SourceBackend {
    let config = ValidatedExtension {
        id: "malformed".into(),
        name: "Malformed".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        popular: Some(ValidatedPopular::Full(Box::new(popular_endpoint()))),
        ..Default::default()
    };
    SourceBackend::Yaml(Box::new(
        YamlSource::new(
            Arc::new(config),
            kani_core::http::SmartClient::new(None)
                .unwrap()
                .with_allow_loopback_egress(true),
            Arc::new(kani_core::cache::InMemoryCache::new()),
            "test:".into(),
            HashMap::new(),
            true,
        )
        .with_eval_limits(limits),
    ))
}

fn items(n: usize) -> String {
    let rows: String = (0..n)
        .map(|i| {
            format!(r#"<div class="item" data-id="m{i}"><span class="title">T{i}</span></div>"#)
        })
        .collect();
    format!("<html><body>{rows}</body></html>")
}

#[tokio::test]
async fn a_listing_past_the_row_cap_is_refused() {
    let origin = TestOrigin::start().await;
    origin.set("/popular", Response::html(&items(12)));
    let backend = source(
        &origin,
        EvalLimits {
            max_list_size: 5,
            ..EvalLimits::default()
        },
    );

    let res = backend.get_popular_manga(1, 50, &[]).await;
    assert!(
        res.is_err(),
        "a 12-row listing must be refused at a cap of 5, got {} rows",
        res.map(|l| l.manga.len()).unwrap_or(0)
    );

    origin.set("/popular", Response::html(&items(4)));
    let ok = source(
        &origin,
        EvalLimits {
            max_list_size: 5,
            ..EvalLimits::default()
        },
    );
    assert_eq!(
        ok.get_popular_manga(1, 50, &[]).await.unwrap().manga.len(),
        4
    );
}

#[tokio::test]
async fn a_missing_required_field_is_an_error() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/popular",
        Response::html(r#"<html><body><div class="item" data-id="m1"></div></body></html>"#),
    );
    let backend = source(&origin, EvalLimits::default());

    let res = backend.get_popular_manga(1, 50, &[]).await;
    assert!(
        res.is_err(),
        "a row missing the required title must error, got {:?}",
        res.map(|l| l.manga.len())
    );
}

#[tokio::test]
async fn an_astral_plane_id_round_trips() {
    let origin = TestOrigin::start().await;
    let id = "漫画-🎉-𝕏";
    origin.set(
        "/popular",
        Response::html(&format!(
            r#"<html><body><div class="item" data-id="{id}"><span class="title">T</span></div></body></html>"#
        )),
    );
    let backend = source(&origin, EvalLimits::default());

    let list = backend.get_popular_manga(1, 50, &[]).await.unwrap();
    assert_eq!(list.manga.len(), 1);
    assert_eq!(list.manga[0].id, id, "the multibyte id survived extraction");
}

#[tokio::test]
async fn an_enormous_field_value_is_refused() {
    let origin = TestOrigin::start().await;
    let big = "x".repeat(200);
    origin.set(
        "/popular",
        Response::html(&format!(
            r#"<html><body><div class="item" data-id="m1"><span class="title">{big}</span></div></body></html>"#
        )),
    );
    let backend = source(
        &origin,
        EvalLimits {
            max_string_length: 32,
            ..EvalLimits::default()
        },
    );

    let res = backend.get_popular_manga(1, 50, &[]).await;
    assert!(
        res.is_err(),
        "a 200-char field must be refused at a cap of 32, got {:?}",
        res.map(|l| l.manga.len())
    );
}

#[tokio::test]
async fn the_default_max_string_length_is_enforced_on_a_live_response() {
    let origin = TestOrigin::start().await;
    let big = "x".repeat(2 * 1024 * 1024);
    origin.set(
        "/popular",
        Response::html(&format!(
            r#"<html><body><div class="item" data-id="m1"><span class="title">{big}</span></div></body></html>"#
        )),
    );
    let backend = source(&origin, EvalLimits::default());

    let res = backend.get_popular_manga(1, 50, &[]).await;
    assert!(
        res.is_err(),
        "a 2 MB field must be refused by the default 1 MB ceiling, got {:?}",
        res.map(|l| l.manga.len())
    );
}

#[tokio::test]
async fn a_row_missing_its_id_is_reported_not_dropped() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/popular",
        Response::html(
            r#"<html><body>
            <div class="item"><span class="title">No Id</span></div>
            <div class="item" data-id="m2"><span class="title">Has Id</span></div>
            </body></html>"#,
        ),
    );
    let backend = source(&origin, EvalLimits::default());

    let res = backend.get_popular_manga(1, 50, &[]).await;
    assert!(
        res.is_err(),
        "a row with a missing id must be reported, not silently dropped (got {:?})",
        res.map(|l| l.manga.len())
    );
}

fn json_field(name: &str, ptr: &str) -> ValidatedField {
    ValidatedField {
        name: name.to_string(),
        source: FieldSource::Blueprint(Expr::SelfRef.ptr(ptr)),
        optional: false,
    }
}

fn json_endpoint(route: &str, container: &str, fields: Vec<ValidatedField>) -> ValidatedEndpoint {
    ValidatedEndpoint {
        route: route.into(),
        response_type: ResponseType::Json,
        container: container.into(),
        fields,
        ..popular_endpoint()
    }
}

/// A source whose popular endpoint parses JSON (`/items` array of `{id,title}`).
fn json_source(origin: &TestOrigin) -> SourceBackend {
    let config = ValidatedExtension {
        id: "malformed".into(),
        name: "Malformed".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        popular: Some(ValidatedPopular::Full(Box::new(json_endpoint(
            "/popular",
            "/items",
            vec![json_field("id", "/id"), json_field("title", "/title")],
        )))),
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

/// A source with a JSON chapter_list endpoint (`/chapters` array of `{id,number}`).
fn chapter_source(origin: &TestOrigin) -> SourceBackend {
    let config = ValidatedExtension {
        id: "malformed".into(),
        name: "Malformed".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        chapter_list: Some(json_endpoint(
            "/chapters",
            "/chapters",
            vec![json_field("id", "/id"), json_field("number", "/number")],
        )),
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

#[tokio::test]
async fn a_non_json_body_from_a_json_endpoint_is_an_error() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/popular",
        Response::html("<html><body>definitely not json</body></html>"),
    );
    let backend = json_source(&origin);

    let res = backend.get_popular_manga(1, 50, &[]).await;
    assert!(
        res.is_err(),
        "HTML from a JSON endpoint must be a parse error"
    );
}

#[tokio::test]
async fn a_container_object_where_an_array_is_expected_is_an_error() {
    let origin = TestOrigin::start().await;
    origin.set("/popular", Response::json(r#"{"items": {"a": 1}}"#));
    let backend = json_source(&origin);

    let res = backend.get_popular_manga(1, 50, &[]).await;
    assert!(
        res.is_err(),
        "an object where the row array should be must error, got {:?}",
        res.map(|l| l.manga.len())
    );
}

#[tokio::test]
async fn a_chapter_with_a_numeric_id_is_reported_not_silently_dropped() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/chapters",
        Response::json(r#"{"chapters": [{"id": 12, "number": 1}, {"id": "c2", "number": 2}]}"#),
    );
    let backend = chapter_source(&origin);

    let list = backend.get_chapter_list("m1", 1, None, None).await.unwrap();
    let ids: Vec<&str> = list.chapters.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["12", "c2"],
        "a numeric id is coerced to its string form, not silently dropped"
    );
}

#[tokio::test]
async fn a_non_finite_chapter_number_is_rejected() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/chapters",
        Response::json(
            r#"{"chapters": [{"id": "c1", "number": "NaN"}, {"id": "c2", "number": "inf"}]}"#,
        ),
    );
    let backend = chapter_source(&origin);

    let list = backend.get_chapter_list("m1", 1, None, None).await.unwrap();
    assert!(
        list.chapters.iter().all(|c| c.number.is_finite()),
        "no chapter may carry a non-finite number, got {:?}",
        list.chapters.iter().map(|c| c.number).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_negative_chapter_number_is_preserved_not_zeroed() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/chapters",
        Response::json(r#"{"chapters": [{"id": "c1", "number": -1}]}"#),
    );
    let backend = chapter_source(&origin);

    let list = backend.get_chapter_list("m1", 1, None, None).await.unwrap();
    assert_eq!(
        list.chapters[0].number, -1.0,
        "a negative number is preserved as the source gave it"
    );
}

#[tokio::test]
async fn a_string_encoded_chapter_number_is_parsed_not_zeroed() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/chapters",
        Response::json(r#"{"chapters": [{"id": "c1", "number": "12.5"}]}"#),
    );
    let backend = chapter_source(&origin);

    let list = backend.get_chapter_list("m1", 1, None, None).await.unwrap();
    assert_eq!(list.chapters.len(), 1);
    assert_eq!(
        list.chapters[0].number, 12.5,
        "a string-encoded number must parse, not become 0.0"
    );
}
