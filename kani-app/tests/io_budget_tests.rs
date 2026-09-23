#![allow(clippy::unwrap_used)]

//! Per-call HTTP I/O budgeting against the
//! interpreted `YamlSource`. Each container row carries a `for_each` sub-fetch,
//! so a listing wider than the budget forces the extraction over the cap. The
//! budget lives on a fresh `HostState` built per call, so it is per-call, not a
//! cumulative per-source counter.

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

fn self_text_field(name: &str) -> ValidatedField {
    ValidatedField {
        name: name.to_string(),
        source: FieldSource::Blueprint(Expr::Text {
            target: Box::new(Expr::SelfRef),
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

/// A `for_each` step that fetches a per-row detail document via the
/// `manga_details` sub-endpoint. Every row costs one I/O charge.
fn detail_for_each(url: &str, policy: OnFailurePolicy) -> ValidatedForEachStep {
    ValidatedForEachStep {
        url_expr: Expr::Literal(url.to_string()),
        merge_as: "detail".into(),
        endpoint_name: "manga_details".into(),
        on_failure: policy,
        deduplicate_by: None,
    }
}

/// A source whose popular endpoint has `container` rows, each triggering a
/// `for_each` sub-fetch to `{origin}/detail`. `manga_details` is defined so the
/// for_each resolves; its body is never reached once the budget trips.
fn source_with_for_each(origin: &TestOrigin, policy: OnFailurePolicy) -> SourceBackend {
    let mut popular = base_endpoint(
        ".item",
        vec![attr_field("id", "data-id"), attr_field("title", "data-id")],
    );
    popular.for_each_steps = vec![detail_for_each(&origin.url("/detail"), policy)];

    let config = ValidatedExtension {
        id: "budget".into(),
        name: "Budget".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        popular: Some(ValidatedPopular::Full(Box::new(popular))),
        manga_details: Some(base_endpoint(".d", vec![self_text_field("x")])),
        ..Default::default()
    };
    SourceBackend::Yaml(Box::new(YamlSource::new(
        Arc::new(config),
        kani_core::http::SmartClient::new(None).unwrap(),
        Arc::new(kani_core::cache::InMemoryCache::new()),
        "test:".into(),
        HashMap::new(),
        true,
    )))
}

fn items(n: usize) -> String {
    let rows: String = (0..n)
        .map(|i| format!(r#"<div class="item" data-id="m{i}"></div>"#))
        .collect();
    format!("<html><body>{rows}</body></html>")
}

#[tokio::test]
async fn the_io_budget_is_enforced_per_call_not_per_source() {
    let origin = TestOrigin::start().await;
    origin.set("/detail", Response::html(r#"<div class="d">ok</div>"#));

    origin.set("/popular", Response::html(&items(40)));
    let backend = source_with_for_each(&origin, OnFailurePolicy::Fail);
    let over = backend.get_popular_manga(1, 50, &[]).await;
    assert!(
        over.is_err(),
        "a 40-row listing must exceed the 32-request budget, got {:?} rows",
        over.map(|l| l.manga.len())
    );

    origin.set("/popular", Response::html(&items(4)));
    let under = backend.get_popular_manga(1, 50, &[]).await;
    assert!(
        under.is_ok(),
        "a 4-row listing on the same source must succeed — the budget is per call: {under:?}"
    );
    assert_eq!(under.unwrap().manga.len(), 4);
}

#[tokio::test]
async fn a_page_set_exceeding_the_io_budget_is_refused_not_truncated() {
    let origin = TestOrigin::start().await;
    origin.set("/detail", Response::html(r#"<div class="d">ok</div>"#));

    origin.set("/popular", Response::html(&items(60)));
    let backend = source_with_for_each(&origin, OnFailurePolicy::Fail);

    let res = backend.get_popular_manga(1, 100, &[]).await;
    assert!(
        res.is_err(),
        "60 per-row sub-fetches must overrun the 32 budget and error, not return a short set of {:?}",
        res.map(|l| l.manga.len())
    );
}
