#![allow(clippy::unwrap_used)]

use kani_core::evaluator::{html_eval::extract_html, json_eval::extract_json};
use kani_core::wasm::{AllowedHost, HostState};
use kani_shared::ast::{BlueprintBuilder, Expr, RequestDef};
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
async fn json_fetch_list_then_detail() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                r#"[{"id":1,"detail_url":"/detail/1"},{"id":2,"detail_url":"/detail/2"},{"id":3,"detail_url":"/detail/3"}]"#,
            ),
        )
        .mount(&server)
        .await;

    for i in 1..=3 {
        let body = format!(r#"{{"id":{i},"title":"Title {i}"}}"#);
        Mock::given(method("GET"))
            .and(wiremock::matchers::path(format!("/detail/{i}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
    }

    let detail_bp = BlueprintBuilder::new("")
        .field("title", Expr::self_ref().ptr("/title").str_val())
        .build();

    let list_bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field("id", Expr::self_ref().ptr("/id").int_val())
        .field(
            "detail",
            Expr::fetch_json(
                Expr::format(
                    "{}{}",
                    vec![
                        Expr::lit(server.uri()),
                        Expr::self_ref().ptr("/detail_url").str_val(),
                    ],
                ),
                detail_bp,
            ),
        )
        .build();

    let base_url = server.uri();
    let mut state = make_state(AllowedHost::Restricted(base_url.clone()));
    let result = extract_json(&mut state, None, &list_bp).await.unwrap();

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "expected 3 rows");
    assert_eq!(rows[0]["id"], 1);
    assert_eq!(rows[0]["detail"]["title"], "Title 1");
    assert_eq!(rows[1]["detail"]["title"], "Title 2");
    assert_eq!(rows[2]["detail"]["title"], "Title 3");
    assert_eq!(state.io_count, 4, "1 list fetch + 3 detail fetches = 4");
}

#[tokio::test]
async fn html_fetch_sub_blueprint() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<ul><li><a href="/item/1">A</a></li><li><a href="/item/2">B</a></li></ul>"#,
        ))
        .mount(&server)
        .await;

    for (i, name) in [(1, "Detail A"), (2, "Detail B")] {
        let body = format!(r#"<html><body><h1>{name}</h1></body></html>"#);
        Mock::given(method("GET"))
            .and(wiremock::matchers::path(format!("/item/{i}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
    }

    let detail_bp = BlueprintBuilder::new(":root")
        .field("heading", Expr::dom("h1").text())
        .build();

    let list_bp = BlueprintBuilder::new("li")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field("href", Expr::self_ref().first("a").attr("href"))
        .field(
            "detail",
            Expr::fetch_html(
                Expr::format(
                    "{}{}",
                    vec![
                        Expr::lit(server.uri()),
                        Expr::self_ref().first("a").attr("href"),
                    ],
                ),
                detail_bp,
            ),
        )
        .build();

    let base_url = server.uri();
    let mut state = make_state(AllowedHost::Restricted(base_url.clone()));
    let result = extract_html(&mut state, None, &list_bp).await.unwrap();

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["detail"]["heading"], "Detail A");
    assert_eq!(rows[1]["detail"]["heading"], "Detail B");
    assert_eq!(state.io_count, 3, "1 list + 2 detail fetches = 3");
}

#[tokio::test]
async fn fetch_disallowed_host_is_rejected() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"[{"url":"https://evil.example.com/page"}]"#),
        )
        .mount(&server)
        .await;

    let detail_bp = BlueprintBuilder::new("").build();
    let list_bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field(
            "data",
            Expr::fetch_json(Expr::self_ref().ptr("/url").str_val(), detail_bp),
        )
        .build();

    let base_url = server.uri();
    let mut state = make_state(AllowedHost::Restricted(base_url.clone()));
    let err = extract_json(&mut state, None, &list_bp).await.unwrap_err();
    assert!(
        err.contains("blocked") || err.contains("only contact"),
        "expected host restriction error, got: {err}"
    );
}

#[tokio::test]
async fn nested_fetch_is_rejected() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"[{"inner_url":"http://example.com/inner"}]"#),
        )
        .mount(&server)
        .await;

    let innermost_bp = BlueprintBuilder::new("").build();
    let inner_bp = BlueprintBuilder::new("")
        .field(
            "nested",
            Expr::fetch_json(Expr::self_ref().ptr("/inner_url").str_val(), innermost_bp),
        )
        .build();

    let outer_bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: format!("{}/", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field(
            "data",
            Expr::fetch_json(Expr::self_ref().ptr("/inner_url").str_val(), inner_bp),
        )
        .build();

    let mut state = make_state(AllowedHost::Unrestricted);
    let err = extract_json(&mut state, None, &outer_bp).await.unwrap_err();
    assert!(
        err.contains("Nested") || err.contains("not allowed"),
        "expected nested Fetch error, got: {err}"
    );
}

#[tokio::test]
async fn fetch_budget_exceeded_after_32_requests() {
    let server = MockServer::start().await;

    let list_items: String = (0..32)
        .map(|i| format!(r#"{{"url":"/item/{i}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let list_body = format!("[{list_items}]");

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(list_body))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"x":1}"#))
        .mount(&server)
        .await;

    let detail_bp = BlueprintBuilder::new("").build();
    let list_bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field(
            "data",
            Expr::fetch_json(
                Expr::format(
                    "{}{}",
                    vec![
                        Expr::lit(server.uri()),
                        Expr::self_ref().ptr("/url").str_val(),
                    ],
                ),
                detail_bp,
            ),
        )
        .build();

    let mut state = make_state(AllowedHost::Unrestricted);
    let err = extract_json(&mut state, None, &list_bp).await.unwrap_err();
    assert!(
        err.contains("maximum") || err.contains("exceeded"),
        "expected budget exceeded error, got: {err}"
    );
}

#[tokio::test]
async fn on_failure_skip_produces_null() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"[{"id":1,"bad_url":"/no-such-path"}]"#),
        )
        .mount(&server)
        .await;

    let detail_bp = BlueprintBuilder::new("")
        .field("x", Expr::self_ref().ptr("/x").str_val())
        .build();

    let list_bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field("id", Expr::self_ref().ptr("/id").int_val())
        .field_opt(
            "detail",
            Expr::fetch_json(
                Expr::format(
                    "{}{}",
                    vec![
                        Expr::lit(server.uri()),
                        Expr::self_ref().ptr("/bad_url").str_val(),
                    ],
                ),
                detail_bp,
            )
            .with_on_failure(kani_shared::ast::OnFailurePolicy::Skip),
        )
        .build();

    let base_url = server.uri();
    let mut state = make_state(AllowedHost::Restricted(base_url.clone()));
    let result = extract_json(&mut state, None, &list_bp).await.unwrap();
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], 1);
    assert!(
        rows[0]["detail"].is_null(),
        "expected null on skip, got: {:?}",
        rows[0]["detail"]
    );
}

#[tokio::test]
async fn on_failure_fail_propagates_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"[{"id":1}]"#))
        .mount(&server)
        .await;

    let detail_bp = BlueprintBuilder::new("")
        .field("x", Expr::self_ref().ptr("/x").str_val())
        .build();

    let list_bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field(
            "detail",
            Expr::fetch_json(Expr::lit(format!("{}/missing", server.uri())), detail_bp),
        )
        .build();

    let base_url = server.uri();
    let mut state = make_state(AllowedHost::Restricted(base_url.clone()));
    let result = extract_json(&mut state, None, &list_bp).await;
    assert!(result.is_err(), "expected error to propagate");
}

#[tokio::test]
async fn on_failure_use_evaluates_fallback() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"[{"id":1}]"#))
        .mount(&server)
        .await;

    let detail_bp = BlueprintBuilder::new("")
        .field("x", Expr::self_ref().ptr("/x").str_val())
        .build();

    let list_bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field("id", Expr::self_ref().ptr("/id").int_val())
        .field(
            "detail",
            Expr::fetch_json(Expr::lit(format!("{}/missing", server.uri())), detail_bp)
                .with_on_failure(kani_shared::ast::OnFailurePolicy::Use(Box::new(Expr::lit(
                    "fallback_value",
                )))),
        )
        .build();

    let base_url = server.uri();
    let mut state = make_state(AllowedHost::Restricted(base_url.clone()));
    let result = extract_json(&mut state, None, &list_bp).await.unwrap();
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["detail"], "fallback_value");
}

#[tokio::test]
async fn html_sub_fetches_run_concurrently_not_sequentially() {
    use std::time::Duration;

    let server = MockServer::start().await;
    const N: usize = 5;
    const DELAY_MS: u64 = 200;

    let list_items: String = (1..=N)
        .map(|i| format!(r#"<li><a href="/item/{i}">Item {i}</a></li>"#))
        .collect();
    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("<ul>{list_items}</ul>")))
        .mount(&server)
        .await;

    for i in 1..=N {
        let body = format!(r#"<html><body><h1>Detail {i}</h1></body></html>"#);
        Mock::given(method("GET"))
            .and(wiremock::matchers::path(format!("/item/{i}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(body)
                    .set_delay(Duration::from_millis(DELAY_MS)),
            )
            .mount(&server)
            .await;
    }

    let detail_bp = BlueprintBuilder::new(":root")
        .field("heading", Expr::dom("h1").text())
        .build();

    let list_bp = BlueprintBuilder::new("li")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field(
            "detail",
            Expr::fetch_html(
                Expr::format(
                    "{}{}",
                    vec![
                        Expr::lit(server.uri()),
                        Expr::self_ref().first("a").attr("href"),
                    ],
                ),
                detail_bp,
            ),
        )
        .build();

    let base_url = server.uri();
    let mut state = make_state(AllowedHost::Restricted(base_url.clone()));

    let started = std::time::Instant::now();
    let result = extract_html(&mut state, None, &list_bp).await.unwrap();
    let elapsed = started.elapsed();

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), N);
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row["detail"]["heading"], format!("Detail {}", i + 1));
    }
    assert_eq!(
        state.io_count as usize,
        N + 1,
        "1 list fetch + N detail fetches"
    );

    assert!(
        elapsed < Duration::from_millis(DELAY_MS * (N as u64) / 2),
        "expected concurrent fan-out to run in well under {}ms, took {:?}",
        DELAY_MS * (N as u64),
        elapsed
    );
}

#[tokio::test]
async fn json_sub_fetches_run_concurrently_not_sequentially() {
    use std::time::Duration;

    let server = MockServer::start().await;
    const N: usize = 5;
    const DELAY_MS: u64 = 200;

    let list_body = format!(
        "[{}]",
        (1..=N)
            .map(|i| format!(r#"{{"id":{i},"detail_url":"/detail/{i}"}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(list_body))
        .mount(&server)
        .await;

    for i in 1..=N {
        let body = format!(r#"{{"id":{i},"title":"Title {i}"}}"#);
        Mock::given(method("GET"))
            .and(wiremock::matchers::path(format!("/detail/{i}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(body)
                    .set_delay(Duration::from_millis(DELAY_MS)),
            )
            .mount(&server)
            .await;
    }

    let detail_bp = BlueprintBuilder::new("")
        .field("title", Expr::self_ref().ptr("/title").str_val())
        .build();

    let list_bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field(
            "detail",
            Expr::fetch_json(
                Expr::format(
                    "{}{}",
                    vec![
                        Expr::lit(server.uri()),
                        Expr::self_ref().ptr("/detail_url").str_val(),
                    ],
                ),
                detail_bp,
            ),
        )
        .build();

    let base_url = server.uri();
    let mut state = make_state(AllowedHost::Restricted(base_url.clone()));

    let started = std::time::Instant::now();
    let result = extract_json(&mut state, None, &list_bp).await.unwrap();
    let elapsed = started.elapsed();

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), N);
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row["detail"]["title"], format!("Title {}", i + 1));
    }
    assert_eq!(
        state.io_count as usize,
        N + 1,
        "1 list fetch + N detail fetches"
    );

    assert!(
        elapsed < Duration::from_millis(DELAY_MS * (N as u64) / 2),
        "expected concurrent fan-out to run in well under {}ms, took {:?}",
        DELAY_MS * (N as u64),
        elapsed
    );
}

/// A server on `127.0.0.2`: another host than wiremock's `127.0.0.1`, yet still loopback.
async fn other_host_serving(body: &'static str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.2:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    format!("http://{addr}/landing")
}

async fn redirecting_to(target: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/start"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", target))
        .mount(&server)
        .await;
    server
}

fn start_request(server: &MockServer) -> RequestDef {
    RequestDef {
        url: format!("{}/start", server.uri()),
        method: "GET".into(),
        headers: vec![],
        queries: vec![],
        endpoint_id: None,
    }
}

#[tokio::test]
async fn a_redirect_is_held_to_the_sources_host() {
    let target = other_host_serving(r#"{"title":"elsewhere"}"#).await;
    let server = redirecting_to(&target).await;
    let bp = BlueprintBuilder::new("")
        .with_request(start_request(&server))
        .field("title", Expr::self_ref().ptr("/title").str_val())
        .build();

    let mut restricted = make_state(AllowedHost::Restricted(server.uri()));
    let err = extract_json(&mut restricted, None, &bp).await.unwrap_err();
    assert!(
        err.contains("redirect"),
        "refused at the redirect, got: {err}"
    );

    let mut unrestricted = make_state(AllowedHost::Unrestricted);
    let out = extract_json(&mut unrestricted, None, &bp).await.unwrap();
    assert_eq!(out["rows"][0]["title"], "elsewhere");
}

#[tokio::test]
async fn a_sub_fetch_redirect_is_held_to_the_sources_host() {
    let target = other_host_serving(r#"{"title":"elsewhere"}"#).await;
    let server = redirecting_to(&target).await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(format!(r#"[{{"url":"{}/start"}}]"#, server.uri())),
        )
        .mount(&server)
        .await;
    let detail_bp = BlueprintBuilder::new("")
        .field("title", Expr::self_ref().ptr("/title").str_val())
        .build();
    let list_bp = BlueprintBuilder::new("")
        .with_request(RequestDef {
            url: format!("{}/list", server.uri()),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field(
            "data",
            Expr::fetch_json(Expr::self_ref().ptr("/url").str_val(), detail_bp),
        )
        .build();

    let mut restricted = make_state(AllowedHost::Restricted(server.uri()));
    let err = extract_json(&mut restricted, None, &list_bp)
        .await
        .unwrap_err();
    assert!(
        err.contains("redirect"),
        "refused at the redirect, got: {err}"
    );
}
