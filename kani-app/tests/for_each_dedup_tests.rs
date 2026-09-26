#![allow(clippy::unwrap_used)]

//! KANI-51: `for_each`'s `deduplicate_by` key.
//!
//! The key is evaluated against each main-result row once its sub-fetch has
//! merged, and rows repeating a key are dropped, first occurrence kept. It was
//! parsed and validated but never read, so a source that repeats a title across
//! categories delivered the duplicates into the user's library.

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::source::{SourceBackend, YamlSource};
use kani_shared::ast::{Expr, OnFailurePolicy};
use kani_shared_test::origin::{Response, TestOrigin};
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedForEachStep,
    ValidatedHnp, ValidatedPopular, ValidatedTotalPages,
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

fn base_endpoint(container: &str, fields: Vec<ValidatedField>) -> ValidatedEndpoint {
    ValidatedEndpoint {
        route: "/popular".into(),
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

/// `deduplicate_by` keyed on the row's own `id` field.
fn dedup_for_each(url: &str, key: Option<Expr>) -> ValidatedForEachStep {
    ValidatedForEachStep {
        url_expr: Expr::Literal(url.to_string()),
        merge_as: "detail".into(),
        endpoint_name: "manga_details".into(),
        on_failure: OnFailurePolicy::Fail,
        deduplicate_by: key,
    }
}

fn source(origin: &TestOrigin, key: Option<Expr>) -> SourceBackend {
    let mut popular = base_endpoint(
        ".item",
        vec![attr_field("id", "data-id"), attr_field("title", "data-id")],
    );
    popular.for_each_steps = vec![dedup_for_each(&origin.url("/detail"), key)];

    let config = ValidatedExtension {
        id: "dedup".into(),
        name: "Dedup".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        popular: Some(ValidatedPopular::Full(Box::new(popular))),
        manga_details: Some(base_endpoint(".d", vec![attr_field("x", "data-x")])),
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

/// Five rows, three distinct ids: m0, m1, m0, m1, m2.
fn repeating_items() -> String {
    let rows: String = ["m0", "m1", "m0", "m1", "m2"]
        .iter()
        .map(|id| format!(r#"<div class="item" data-id="{id}"></div>"#))
        .collect();
    format!("<html><body>{rows}</body></html>")
}

async fn ids_from(origin: &TestOrigin, key: Option<Expr>) -> Vec<String> {
    origin.set(
        "/detail",
        Response::html(r#"<div class="d" data-x="ok"></div>"#),
    );
    origin.set("/popular", Response::html(&repeating_items()));
    let backend = source(origin, key);
    backend
        .get_popular_manga(1, 50, &[])
        .await
        .expect("listing should succeed")
        .manga
        .into_iter()
        .map(|m| m.id)
        .collect()
}

#[tokio::test]
async fn a_repeated_key_drops_the_later_row() {
    let origin = TestOrigin::start().await;
    let ids = ids_from(&origin, Some(Expr::SelfRef.ptr("/id"))).await;

    assert_eq!(
        ids,
        vec!["m0", "m1", "m2"],
        "rows repeating a key must collapse to the first occurrence, in order"
    );
}

#[tokio::test]
async fn without_a_key_every_row_survives() {
    let origin = TestOrigin::start().await;
    let ids = ids_from(&origin, None).await;

    assert_eq!(
        ids,
        vec!["m0", "m1", "m0", "m1", "m2"],
        "a step with no deduplicate_by must not filter anything"
    );
}
