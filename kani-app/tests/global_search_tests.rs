#![allow(clippy::unwrap_used)]

//! Global search across several sources, driven against real `YamlSource`
//! backends so a source that fails fails the way production makes it fail.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::source::{SourceBackend, YamlSource};
use kani_shared::ast::Expr;
use kani_shared::types::SearchScope;
use kani_shared_test::origin::{Response, TestOrigin};
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
    ValidatedTotalPages,
};
use kani_yaml::yaml::schema::ResponseType;

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

fn backend(origin: &TestOrigin, id: &str, route: &str) -> SourceBackend {
    let config = ValidatedExtension {
        id: id.into(),
        name: id.into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        search: Some(ValidatedEndpoint {
            route: route.into(),
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
        }),
        ..Default::default()
    };
    SourceBackend::Yaml(Box::new(YamlSource::new(
        Arc::new(config),
        kani_core::http::SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true),
        Arc::new(kani_core::cache::InMemoryCache::new()),
        format!("{id}:"),
        HashMap::new(),
        true,
    )))
}

#[tokio::test]
async fn a_source_that_did_not_answer_is_not_reported_as_one_with_no_matches() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/hit",
        Response::html(
            r#"<div class="item" data-id="a1"><span class="title">Paper Cranes</span></div>"#,
        ),
    );
    // A page the container selector finds nothing in: answered, matched nothing.
    origin.set("/none", Response::html("<div class=\"other\"></div>"));
    origin.set("/broken", Response::status(500));

    let svc = common::test_service().await;
    for (name, route) in [
        ("hit", "/hit?q=$query$"),
        ("none", "/none?q=$query$"),
        ("broken", "/broken?q=$query$"),
    ] {
        let id = common::insert_source(&svc.db, name).await;
        sqlx::query("UPDATE sources SET enabled = 1 WHERE id = ?")
            .bind(id)
            .execute(&svc.db)
            .await
            .unwrap();
        svc.sources.insert(id, backend(&origin, name, route));
    }

    let results = svc
        .global_search("cranes", SearchScope::AllEnabled, 1, 24)
        .await
        .unwrap();

    let by_name = |n: &str| {
        results
            .iter()
            .find(|r| r.source_name == n)
            .unwrap_or_else(|| panic!("{n} missing from results"))
    };

    let hit = by_name("hit");
    assert_eq!(hit.manga.len(), 1, "the working source found its manga");
    assert!(
        hit.error.is_none(),
        "a source that answered carries no error"
    );

    let none = by_name("none");
    assert!(none.manga.is_empty());
    assert!(
        none.error.is_none(),
        "a source that answered with nothing has not failed"
    );

    let broken = by_name("broken");
    assert!(broken.manga.is_empty());
    assert!(
        broken.error.is_some(),
        "a source that did not answer must be distinguishable from one that found nothing"
    );
}
