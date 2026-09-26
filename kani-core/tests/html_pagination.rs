#![allow(clippy::unwrap_used)]

//! Host-side pagination over an HTML source whose native chunk size differs from
//! the page size the caller asked for.
//!
//! `extract_html_paginated` is what every extension's list endpoint goes through,
//! and mutation testing found it survived being replaced with a stub returning a
//! default. These tests assert the stitched rows, the offsets actually requested,
//! and where the walk stops.

use kani_core::evaluator::html_eval::extract_html_paginated;
use kani_core::wasm::{AllowedHost, HostState};
use kani_shared::ast::{Blueprint, BlueprintBuilder, Expr, OffsetType, RequestDef};
use std::sync::Arc;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_state(allowed: AllowedHost) -> HostState {
    let client = kani_core::http::SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true);
    HostState::new(
        client,
        allowed,
        Arc::new(kani_core::cache::InMemoryCache::new()),
        String::new(),
        kani_core::v8_process::new_handle(),
    )
    .unwrap()
}

/// One `<li>` per item, numbered from `first` so a stitched result can be checked
/// against the source's own numbering rather than its position in the output.
fn chunk_html(first: usize, count: usize) -> String {
    let items: String = (first..first + count)
        .map(|n| format!("<li>item {n}</li>"))
        .collect();
    format!("<html><body><ul>{items}</ul></body></html>")
}

async fn serve_chunk(server: &MockServer, param: &str, value: &str, body: String) {
    Mock::given(method("GET"))
        .and(path("/list"))
        .and(query_param(param, value))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(server)
        .await;
}

fn paginated_blueprint(
    server: &MockServer,
    native_page_size: usize,
    offset_param: &str,
    offset_type: OffsetType,
) -> Blueprint {
    BlueprintBuilder::new("li")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field("t", Expr::self_ref().text())
        .paginated(native_page_size, offset_param, offset_type)
        .build()
}

fn titles(result: &serde_json::Value) -> Vec<String> {
    result["rows"]
        .as_array()
        .expect("a paginated extraction must return a rows array")
        .iter()
        .map(|row| row["t"].as_str().unwrap().to_owned())
        .collect()
}

/// Offsets in the order the source was asked for them.
async fn requested(server: &MockServer, param: &str) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter_map(|request| {
            request
                .url
                .query_pairs()
                .find(|(key, _)| key == param)
                .map(|(_, value)| value.into_owned())
        })
        .collect()
}

#[tokio::test]
async fn a_requested_page_is_stitched_from_several_native_chunks() {
    let server = MockServer::start().await;
    for chunk in 0..3 {
        serve_chunk(
            &server,
            "offset",
            &(chunk * 3).to_string(),
            chunk_html(chunk * 3, 3),
        )
        .await;
    }
    let blueprint = paginated_blueprint(&server, 3, "offset", OffsetType::ItemOffset);
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, 1, 7, &blueprint)
        .await
        .unwrap();

    assert_eq!(
        titles(&result),
        (0..7).map(|n| format!("item {n}")).collect::<Vec<_>>(),
        "seven items must come back in source order across three three-item chunks"
    );
    assert_eq!(
        requested(&server, "offset").await,
        vec!["0", "3", "6"],
        "each native chunk must be requested exactly once, in order"
    );
}

#[tokio::test]
async fn a_page_starting_mid_chunk_skips_the_items_before_it() {
    let server = MockServer::start().await;
    for chunk in 0..3 {
        serve_chunk(
            &server,
            "offset",
            &(chunk * 3).to_string(),
            chunk_html(chunk * 3, 3),
        )
        .await;
    }
    let blueprint = paginated_blueprint(&server, 3, "offset", OffsetType::ItemOffset);
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    // Page 3 of size 2 starts at item 4, which is the second item of the chunk at
    // offset 3 — the case where the first chunk is entered partway through.
    let result = extract_html_paginated(&mut state, 3, 2, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&result), vec!["item 4", "item 5"]);
    assert_eq!(
        requested(&server, "offset").await,
        vec!["3"],
        "a page contained in one native chunk must not fetch its neighbours"
    );

    // Item 7 sits in the chunk at offset 6, one item in. The skip is the remainder
    // of the division, not its quotient, and below three native pages the two
    // coincide — so this is the first offset that tells them apart.
    let deep = extract_html_paginated(&mut state, 8, 1, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&deep), vec!["item 7"]);
}

#[tokio::test]
async fn page_number_offsets_are_counted_in_pages_from_the_declared_start() {
    let server = MockServer::start().await;
    for page in 1..=3 {
        serve_chunk(
            &server,
            "p",
            &page.to_string(),
            chunk_html((page - 1) * 2, 2),
        )
        .await;
    }
    let blueprint = paginated_blueprint(&server, 2, "p", OffsetType::PageNumber { start: 1 });
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, 2, 3, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&result), vec!["item 3", "item 4", "item 5"]);
    assert_eq!(
        requested(&server, "p").await,
        vec!["2", "3"],
        "an item offset of 3 with a two-item native page is page 2 when pages start at 1"
    );
}

#[tokio::test]
async fn an_offset_already_in_the_request_is_replaced_rather_than_added() {
    let server = MockServer::start().await;
    serve_chunk(&server, "offset", "0", chunk_html(0, 2)).await;
    let mut blueprint = paginated_blueprint(&server, 2, "offset", OffsetType::ItemOffset);
    blueprint.request.as_mut().unwrap().queries = vec![
        ("offset".into(), "999".into()),
        ("lang".into(), "en".into()),
    ];
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, 1, 2, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&result), vec!["item 0", "item 1"]);
    let sent = server.received_requests().await.unwrap();
    let offsets: Vec<String> = sent[0]
        .url
        .query_pairs()
        .filter(|(key, _)| key == "offset")
        .map(|(_, value)| value.into_owned())
        .collect();
    assert_eq!(
        offsets,
        vec!["0"],
        "the caller's stale offset must be dropped, not sent alongside the computed one"
    );
    assert!(
        sent[0]
            .url
            .query_pairs()
            .any(|(k, v)| k == "lang" && v == "en"),
        "replacing the offset must leave the request's other queries alone"
    );
}

#[tokio::test]
async fn a_short_chunk_ends_the_walk_and_reports_no_next_page() {
    let server = MockServer::start().await;
    serve_chunk(&server, "offset", "0", chunk_html(0, 3)).await;
    // Two items where the native size is three: the source has run out.
    serve_chunk(&server, "offset", "3", chunk_html(3, 2)).await;
    let blueprint = paginated_blueprint(&server, 3, "offset", OffsetType::ItemOffset);
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, 1, 20, &blueprint)
        .await
        .unwrap();

    assert_eq!(
        titles(&result).len(),
        5,
        "both chunks must be returned whole"
    );
    assert_eq!(
        result["scalars"]["has_next_page"], false,
        "a chunk shorter than the native size means there is nothing after it"
    );
    assert_eq!(
        requested(&server, "offset").await,
        vec!["0", "3"],
        "the walk must stop at the short chunk rather than asking for another"
    );
}

#[tokio::test]
async fn a_full_final_chunk_reports_a_next_page() {
    let server = MockServer::start().await;
    serve_chunk(&server, "offset", "0", chunk_html(0, 3)).await;
    let blueprint = paginated_blueprint(&server, 3, "offset", OffsetType::ItemOffset);
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, 1, 3, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&result).len(), 3);
    assert_eq!(
        result["scalars"]["has_next_page"], true,
        "a chunk that filled the native size implies more behind it"
    );
}

/// A source that returns more than its declared native page size is still full.
/// Treating "full" as an exact match would end the walk at the first such chunk.
#[tokio::test]
async fn a_chunk_larger_than_the_declared_native_size_still_counts_as_full() {
    let server = MockServer::start().await;
    serve_chunk(&server, "offset", "0", chunk_html(0, 3)).await;
    serve_chunk(&server, "offset", "2", chunk_html(0, 0)).await;
    let blueprint = paginated_blueprint(&server, 2, "offset", OffsetType::ItemOffset);
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, 1, 10, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&result), vec!["item 0", "item 1", "item 2"]);
    assert_eq!(
        requested(&server, "offset").await,
        vec!["0", "2"],
        "three items against a declared size of two is at least full, so the walk continues"
    );
}

/// The requested page is filled exactly, and the chunk that filled it was short.
/// Nothing else reports on what lies beyond, so the chunk length has to.
#[tokio::test]
async fn a_page_filled_by_a_short_chunk_reports_no_next_page() {
    let server = MockServer::start().await;
    serve_chunk(&server, "offset", "0", chunk_html(0, 2)).await;
    let blueprint = paginated_blueprint(&server, 3, "offset", OffsetType::ItemOffset);
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, 1, 2, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&result), vec!["item 0", "item 1"]);
    assert_eq!(
        result["scalars"]["has_next_page"], false,
        "the page filled, but on a chunk the source could not fill"
    );
    assert_eq!(requested(&server, "offset").await, vec!["0"]);
}

#[tokio::test]
async fn the_sources_own_has_next_page_scalar_overrides_the_chunk_length() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "<html><body><div id='more'>no</div><ul>{}</ul></body></html>",
            (0..3)
                .map(|n| format!("<li>item {n}</li>"))
                .collect::<String>()
        )))
        .mount(&server)
        .await;
    let blueprint = BlueprintBuilder::new("li")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field("t", Expr::self_ref().text())
        .scalar(
            "has_next_page",
            Expr::eq(Expr::dom("#more").text(), Expr::lit("yes")),
        )
        .paginated(3, "offset", OffsetType::ItemOffset)
        .build();
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, 1, 3, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&result).len(), 3);
    assert_eq!(
        result["scalars"]["has_next_page"], false,
        "a full chunk would imply a next page, but the source said otherwise"
    );
}

#[tokio::test]
async fn a_source_that_denies_a_next_page_ends_the_walk_early() {
    let server = MockServer::start().await;
    let body = format!(
        "<html><body><div id='more'>no</div><ul>{}</ul></body></html>",
        (0..3)
            .map(|n| format!("<li>item {n}</li>"))
            .collect::<String>()
    );
    Mock::given(method("GET"))
        .and(path("/list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let blueprint = BlueprintBuilder::new("li")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field("t", Expr::self_ref().text())
        .scalar(
            "has_next_page",
            Expr::eq(Expr::dom("#more").text(), Expr::lit("yes")),
        )
        .paginated(3, "offset", OffsetType::ItemOffset)
        .build();
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    // Twelve items asked for, three available, and the source says there is no more.
    let result = extract_html_paginated(&mut state, 1, 12, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&result).len(), 3);
    assert_eq!(
        requested(&server, "offset").await,
        vec!["0"],
        "a full chunk that denies a next page must not be followed by another request"
    );
}

#[tokio::test]
async fn an_empty_chunk_ends_the_walk() {
    let server = MockServer::start().await;
    serve_chunk(&server, "offset", "0", chunk_html(0, 2)).await;
    serve_chunk(&server, "offset", "2", chunk_html(0, 0)).await;
    let blueprint = paginated_blueprint(&server, 2, "offset", OffsetType::ItemOffset);
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, 1, 10, &blueprint)
        .await
        .unwrap();

    assert_eq!(titles(&result), vec!["item 0", "item 1"]);
    assert_eq!(result["scalars"]["has_next_page"], false);
    assert_eq!(requested(&server, "offset").await, vec!["0", "2"]);
}

#[tokio::test]
async fn a_page_index_below_one_is_treated_as_the_first_page() {
    let server = MockServer::start().await;
    serve_chunk(&server, "offset", "0", chunk_html(0, 2)).await;
    let blueprint = paginated_blueprint(&server, 2, "offset", OffsetType::ItemOffset);
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let result = extract_html_paginated(&mut state, -3, 2, &blueprint)
        .await
        .unwrap();

    assert_eq!(
        titles(&result),
        vec!["item 0", "item 1"],
        "a negative page must clamp to the start rather than computing a negative offset"
    );
    assert_eq!(requested(&server, "offset").await, vec!["0"]);
}

#[tokio::test]
async fn cursor_pagination_is_refused_for_html() {
    let server = MockServer::start().await;
    serve_chunk(&server, "cursor", "0", chunk_html(0, 2)).await;
    let blueprint = paginated_blueprint(
        &server,
        2,
        "cursor",
        OffsetType::CursorToken {
            next_cursor_field: "next".into(),
        },
    );
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let error = extract_html_paginated(&mut state, 1, 2, &blueprint)
        .await
        .unwrap_err();

    assert!(
        error.contains("CursorToken"),
        "the refusal must name the pagination kind it cannot handle, got: {error}"
    );
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "an unsupported pagination kind must be refused before any request is sent"
    );
}

#[tokio::test]
async fn a_blueprint_without_pagination_is_refused() {
    let server = MockServer::start().await;
    serve_chunk(&server, "offset", "0", chunk_html(0, 2)).await;
    let blueprint = BlueprintBuilder::new("li")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field("t", Expr::self_ref().text())
        .build();
    let mut state = make_state(AllowedHost::Restricted(server.uri()));

    let error = extract_html_paginated(&mut state, 1, 2, &blueprint)
        .await
        .unwrap_err();

    assert!(
        error.contains("PaginationConfig"),
        "the refusal must say what the blueprint is missing, got: {error}"
    );
}
