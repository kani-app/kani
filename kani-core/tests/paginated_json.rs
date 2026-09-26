#![allow(clippy::unwrap_used)]

use kani_core::evaluator::json_eval::extract_json_paginated;
use kani_core::wasm::{AllowedHost, HostState};
use kani_shared::ast::{BlueprintBuilder, Expr, OffsetType, RequestDef};
use std::sync::Arc;
use wiremock::matchers::method;
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

#[tokio::test]
async fn offset_pagination_two_pages() {
    let server = MockServer::start().await;

    let items_p1: String = (1usize..=20)
        .map(|i| format!(r#"{{"id":{i}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    Mock::given(method("GET"))
        .and(wiremock::matchers::query_param("offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("[{}]", items_p1)))
        .mount(&server)
        .await;

    let items_p2: String = (21usize..=35)
        .map(|i| format!(r#"{{"id":{i}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    Mock::given(method("GET"))
        .and(wiremock::matchers::query_param("offset", "20"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("[{}]", items_p2)))
        .mount(&server)
        .await;

    let bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: server.uri(),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .paginated(20, "offset", OffsetType::ItemOffset)
        .field("id", Expr::self_ref().ptr("/id").int_val())
        .build();

    let mut state = make_state(AllowedHost::Unrestricted);
    let result = extract_json_paginated(&mut state, 1, 40, &bp)
        .await
        .unwrap();

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 35);
    assert_eq!(rows[0]["id"], 1);
    assert_eq!(rows[34]["id"], 35);
    assert_eq!(result["scalars"]["has_next_page"], false);
}

#[tokio::test]
async fn page_number_pagination() {
    let server = MockServer::start().await;

    for page in [1u32, 2] {
        let start = (page as usize - 1) * 10 + 1;
        let items: String = (start..start + 10)
            .map(|i| format!(r#"{{"n":{i}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        Mock::given(method("GET"))
            .and(wiremock::matchers::query_param("page", page.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!("[{}]", items)))
            .mount(&server)
            .await;
    }

    let bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: server.uri(),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .paginated(10, "page", OffsetType::PageNumber { start: 1 })
        .field("n", Expr::self_ref().ptr("/n").int_val())
        .build();

    let mut state = make_state(AllowedHost::Unrestricted);
    let result = extract_json_paginated(&mut state, 1, 20, &bp)
        .await
        .unwrap();

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 20);
    assert_eq!(rows[0]["n"], 1);
    assert_eq!(rows[19]["n"], 20);
}

#[tokio::test]
async fn cursor_token_pagination_three_chunks() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"items":[{"v":1},{"v":2},{"v":3},{"v":4},{"v":5},{"v":6},{"v":7},{"v":8},{"v":9},{"v":10}],"next":"cur_B"}"#,
        ))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::query_param("cursor", "cur_B"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"items":[{"v":11},{"v":12},{"v":13},{"v":14},{"v":15},{"v":16},{"v":17},{"v":18},{"v":19},{"v":20}],"next":"cur_C"}"#,
        ))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::query_param("cursor", "cur_C"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"items":[{"v":21},{"v":22},{"v":23},{"v":24},{"v":25},{"v":26},{"v":27},{"v":28},{"v":29},{"v":30}]}"#,
        ))
        .mount(&server)
        .await;

    let bp = BlueprintBuilder::new("/items")
        .with_request(RequestDef {
            url: server.uri(),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .paginated(
            10,
            "cursor",
            OffsetType::CursorToken {
                next_cursor_field: "next".into(),
            },
        )
        .scalar_opt("next", Expr::json_root("/next").str_val())
        .field("v", Expr::self_ref().ptr("/v").int_val())
        .build();

    let mut state = make_state(AllowedHost::Unrestricted);
    let result = extract_json_paginated(&mut state, 1, 30, &bp)
        .await
        .unwrap();

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 30, "expected 30 rows across 3 cursor chunks");
    assert_eq!(rows[0]["v"], 1);
    assert_eq!(rows[29]["v"], 30);
    assert_eq!(result["scalars"]["has_next_page"], false);
}

#[tokio::test]
async fn stops_when_has_next_page_false() {
    let server = MockServer::start().await;

    let items: String = (1..=5)
        .map(|i| format!(r#"{{"x":{i}}}"#))
        .collect::<Vec<_>>()
        .join(",");

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(format!(r#"{{"items":[{}],"has_next_page":false}}"#, items)),
        )
        .mount(&server)
        .await;

    let bp = BlueprintBuilder::new("/items")
        .with_request(RequestDef {
            url: server.uri(),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .paginated(20, "offset", OffsetType::ItemOffset)
        .scalar(
            "has_next_page",
            Expr::json_root("/has_next_page").bool_val(),
        )
        .field("x", Expr::self_ref().ptr("/x").int_val())
        .build();

    let mut state = make_state(AllowedHost::Unrestricted);
    let result = extract_json_paginated(&mut state, 1, 40, &bp)
        .await
        .unwrap();

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        5,
        "should stop after first chunk when has_next_page=false"
    );
    assert_eq!(result["scalars"]["has_next_page"], false);
}

/// A paginated endpoint still declares scalars, and `total_pages` is read from
/// one of them. Dropping them leaves the client with a single page.
#[tokio::test]
async fn paginated_extraction_preserves_declared_scalars() {
    let server = MockServer::start().await;
    let items: String = (1usize..=20)
        .map(|i| format!(r#"{{"id":{i}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"{{"items":[{items}],"meta":{{"lastPage":923}}}}"#
        )))
        .mount(&server)
        .await;

    let bp = BlueprintBuilder::new("/items")
        .with_request(RequestDef {
            url: server.uri(),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .paginated(20, "page", OffsetType::PageNumber { start: 1 })
        .field("id", Expr::self_ref().ptr("/id").int_val())
        .scalar("total_pages", Expr::json_root("/meta/lastPage").int_val())
        .build();

    let mut state = make_state(AllowedHost::Unrestricted);
    let result = extract_json_paginated(&mut state, 1, 20, &bp)
        .await
        .unwrap();

    assert_eq!(
        result["scalars"]["total_pages"], 923,
        "declared scalars were dropped: {}",
        result["scalars"]
    );
}

#[test]
fn total_pages_is_rescaled_to_the_requested_page_size() {
    let mut scalars = serde_json::Map::new();
    scalars.insert("total_pages".into(), serde_json::json!(923));
    kani_core::evaluator::json_eval::rescale_total_pages_for_test(&mut scalars, 100, 10);
    assert_eq!(scalars["total_pages"], 9230);
}

#[test]
fn an_item_count_makes_the_page_total_exact() {
    let mut scalars = serde_json::Map::new();
    scalars.insert("total_pages".into(), serde_json::json!(923));
    scalars.insert("total_items".into(), serde_json::json!(92205));
    kani_core::evaluator::json_eval::rescale_total_pages_for_test(&mut scalars, 100, 10);
    assert_eq!(
        scalars["total_pages"], 9221,
        "item count must win over page count"
    );
}

#[test]
fn a_matching_page_size_leaves_the_total_alone() {
    let mut scalars = serde_json::Map::new();
    scalars.insert("total_pages".into(), serde_json::json!(42));
    kani_core::evaluator::json_eval::rescale_total_pages_for_test(&mut scalars, 20, 20);
    assert_eq!(scalars["total_pages"], 42);
}
