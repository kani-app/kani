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
            body: None,
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
            body: None,
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
            body: None,
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
            body: None,
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
async fn a_json_fetch_past_the_operation_request_limit_is_a_budget_error() {
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
            body: None,
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
    state.operation_budget =
        kani_core::budget::OperationBudget::new(kani_core::budget::OperationLimits {
            max_requests: 32,
            ..Default::default()
        });
    let err = extract_json(&mut state, None, &list_bp).await.unwrap_err();
    assert!(
        kani_core::budget::is_budget_exceeded(&err) && err.contains("requests"),
        "expected a request budget error, got: {err}"
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
            body: None,
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
            body: None,
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
            body: None,
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
            body: None,
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
            body: None,
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
        body: None,
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
            body: None,
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

/// Each phase of a chained endpoint reads its own context: row fields and `for_each.url_expr`
/// the container element, the sub-endpoint's fields the sub-page, an `on_failure` fallback the
/// container element again, and a `then` step the main document. The pages disagree on every
/// value, so a phase reading the wrong context produces a visibly wrong row.
#[tokio::test]
async fn each_chaining_phase_reads_its_own_context() {
    let server = MockServer::start().await;
    let base = server.uri();
    let page = |body: String| ResponseTemplate::new(200).set_body_raw(body, "text/html");
    let mount = |p: &str, body: String| {
        Mock::given(method("GET"))
            .and(wiremock::matchers::path(p.to_string()))
            .respond_with(page(body))
    };
    mount(
        "/popular",
        format!(
            r#"<a class="banner" href="{base}/banner"></a>
            <div class="item" data-id="row-1"><span class="t">One</span><a class="link" href="{base}/sub/A"></a></div>
            <div class="item" data-id="row-2"><span class="t">Two</span><a class="link" href="{base}/sub/missing"></a></div>"#
        ),
    )
    .mount(&server)
    .await;
    mount(
        "/sub/A",
        format!(
            r#"<div class="manga"><h1>Sub A</h1><a class="link" href="{base}/sub/WRONG"></a></div>"#
        ),
    )
    .mount(&server)
    .await;
    mount(
        "/banner",
        r#"<div class="manga"><h1>Banner</h1><a class="link" href="/b"></a></div>"#.to_string(),
    )
    .mount(&server)
    .await;

    let yaml = format!(
        r#"id: phases
name: phases
version: "1.0.0"
base_url: "{base}"
endpoints:
  popular:
    route: /popular
    container: ".item"
    fields:
      id: 'self.attr("data-id")'
      title: 'self.first(".t").text()'
      link_seen: 'self.first(".link").attr("href")'
    scalars:
      banner: '$banner'
    then:
      - endpoint: manga_details
        url_expr: 'dom(".banner").attr("href")'
        merge_as: banner
    for_each:
      - endpoint: manga_details
        url_expr: 'self.first(".link").attr("href")'
        merge_as: details
        on_failure: 'self.attr("data-id")'
  manga_details:
    route: "/m/$manga_id$"
    container: ".manga"
    fields:
      id: '"$manga_id$"'
      title: 'self.first("h1").text()'
      status: '"unknown"'
      link: 'self.first(".link").attr("href")'
"#
    );
    let ext = kani_yaml::parse_and_validate(&yaml, std::path::Path::new("phases.yaml")).unwrap();
    let ep = ext.endpoint_by_name("popular").unwrap();
    let bp = kani_yaml::build_blueprint(
        ep,
        &ext,
        "popular",
        RequestDef {
            body: None,
            url: format!("{base}/popular"),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        },
    );
    let mut state = make_state(AllowedHost::Unrestricted);
    let result = extract_html(&mut state, None, &bp).await.unwrap();
    let rows = result["rows"].as_array().unwrap();

    assert_eq!(
        rows[0]["link_seen"],
        format!("{base}/sub/A"),
        "row fields read the container"
    );
    assert_eq!(
        rows[0]["details"]["title"], "Sub A",
        "url_expr read the container link"
    );
    assert_eq!(
        rows[0]["details"]["link"],
        format!("{base}/sub/WRONG"),
        "sub-endpoint fields read the sub-page"
    );
    assert_eq!(
        rows[1]["details"], "row-2",
        "on_failure reads the container element"
    );
    assert_eq!(
        result["scalars"]["banner"]["title"], "Banner",
        "then reads the main document"
    );
    let requested: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect();
    assert!(
        !requested.contains(&"/sub/WRONG".to_string()),
        "{requested:?}"
    );
}

async fn fifty_row_listing() -> (MockServer, kani_shared::ast::Blueprint) {
    let server = MockServer::start().await;
    let base = server.uri();
    let items: String = (0..50)
        .map(|i| format!(r#"<li><a href="{base}/d/{i}">{i}</a></li>"#))
        .collect();
    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("<ul>{items}</ul>")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::path_regex(r"^/d/\d+$"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<h1>Detail page body</h1>"))
        .mount(&server)
        .await;
    let detail = BlueprintBuilder::new(":root")
        .field("heading", Expr::dom("h1").text())
        .build();
    let list = BlueprintBuilder::new("li")
        .with_request(RequestDef {
            body: None,
            url: format!("{base}/list"),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        })
        .field(
            "detail",
            Expr::fetch_html(Expr::self_ref().first("a").attr("href"), detail)
                .with_on_failure(kani_shared::ast::OnFailurePolicy::Skip),
        )
        .build();
    (server, list)
}

#[tokio::test]
async fn a_fifty_row_for_each_fits_the_default_budget() {
    let (_server, list) = fifty_row_listing().await;
    let mut state = make_state(AllowedHost::Unrestricted);
    let result = extract_html(&mut state, None, &list).await.unwrap();
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 50);
    let missing = rows.iter().filter(|r| r["detail"].is_null()).count();
    assert_eq!(missing, 0, "every sub-fetch ran; none was silently nulled");
}

#[tokio::test]
async fn an_exhausted_budget_is_an_error_even_under_on_failure_skip() {
    use kani_core::budget::{OperationBudget, OperationLimits};
    let generous = OperationLimits::default();
    for (limits, needle) in [
        (
            OperationLimits {
                max_requests: 10,
                ..generous
            },
            "requests",
        ),
        (
            OperationLimits {
                max_response_bytes: 400,
                ..generous
            },
            "response bytes",
        ),
        (
            OperationLimits {
                max_elapsed: std::time::Duration::ZERO,
                ..generous
            },
            "time",
        ),
    ] {
        let (_server, list) = fifty_row_listing().await;
        let mut state = make_state(AllowedHost::Unrestricted);
        state.operation_budget = OperationBudget::new(limits);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let err = extract_html(&mut state, None, &list).await.unwrap_err();
        assert!(
            kani_core::budget::is_budget_exceeded(&err) && err.contains(needle),
            "{needle}: {err}"
        );
    }
}

/// A cursor source serving `total` items in native chunks of 32: the cursor is the index of
/// the chunk's first item, and the last chunk has no cursor.
async fn cursor_source(total: usize, repeat_at: Option<usize>) -> MockServer {
    let server = MockServer::start().await;
    for start in (0..total).step_by(32) {
        let end = (start + 32).min(total);
        let items: Vec<serde_json::Value> = (start..end)
            .map(|i| serde_json::json!({ "id": i + 1 }))
            .collect();
        let next = match repeat_at {
            Some(r) if start >= r => serde_json::json!(r.to_string()),
            _ if end < total => serde_json::json!(end.to_string()),
            _ => serde_json::Value::Null,
        };
        let body = serde_json::json!({ "items": items, "next": next }).to_string();
        let matcher = wiremock::matchers::path("/list");
        let mock = if start == 0 {
            Mock::given(method("GET"))
                .and(matcher)
                .and(wiremock::matchers::query_param_is_missing("after"))
        } else {
            Mock::given(method("GET"))
                .and(matcher)
                .and(wiremock::matchers::query_param("after", start.to_string()))
        };
        mock.respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
    }
    server
}

fn cursor_blueprint(base: &str) -> kani_shared::ast::Blueprint {
    let yaml = format!(
        r#"id: cursors
name: cursors
version: "1.0.0"
base_url: "{base}"
endpoints:
  search:
    route: /list
    type: json
    container: /items
    pagination:
      native_page_size: 32
      offset_param: after
      offset_type: cursor
      cursor_field: /next
    fields:
      id: 'self.ptr("/id").int().to_string()'
      title: 'self.ptr("/id").int().to_string()'
"#
    );
    let ext = kani_yaml::parse_and_validate(&yaml, std::path::Path::new("c.yaml")).unwrap();
    let ep = ext.endpoint_by_name("search").unwrap();
    kani_yaml::build_blueprint(
        ep,
        &ext,
        "search",
        RequestDef {
            body: None,
            url: format!("{base}/list"),
            method: "GET".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        },
    )
}

fn ids(result: &serde_json::Value) -> Vec<u64> {
    result["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().parse().unwrap())
        .collect()
}

#[tokio::test]
async fn cursor_pagination_walks_from_the_start_to_the_requested_slice() {
    let server = cursor_source(150, None).await;
    let bp = cursor_blueprint(&server.uri());

    let mut state = make_state(AllowedHost::Unrestricted);
    let page5 = kani_core::evaluator::json_eval::extract_json_paginated(&mut state, 5, 20, &bp)
        .await
        .unwrap();
    assert_eq!(ids(&page5), (81..=100).collect::<Vec<_>>(), "a cold page 5");
    assert_eq!(page5["scalars"]["has_next_page"], true);

    let mut state = make_state(AllowedHost::Unrestricted);
    let page2 = kani_core::evaluator::json_eval::extract_json_paginated(&mut state, 2, 20, &bp)
        .await
        .unwrap();
    assert_eq!(
        ids(&page2),
        (21..=40).collect::<Vec<_>>(),
        "straddles chunks 1 and 2"
    );

    let mut state = make_state(AllowedHost::Unrestricted);
    let last = kani_core::evaluator::json_eval::extract_json_paginated(&mut state, 8, 20, &bp)
        .await
        .unwrap();
    assert_eq!(ids(&last), (141..=150).collect::<Vec<_>>());
    assert_eq!(
        last["scalars"]["has_next_page"], false,
        "the null cursor ends paging"
    );

    let mut state = make_state(AllowedHost::Unrestricted);
    let beyond = kani_core::evaluator::json_eval::extract_json_paginated(&mut state, 9, 20, &bp)
        .await
        .unwrap();
    assert!(ids(&beyond).is_empty());
    assert_eq!(beyond["scalars"]["has_next_page"], false);
}

#[tokio::test]
async fn a_repeated_cursor_ends_the_walk() {
    let server = cursor_source(150, Some(64)).await;
    let bp = cursor_blueprint(&server.uri());
    let mut state = make_state(AllowedHost::Unrestricted);
    let page5 = kani_core::evaluator::json_eval::extract_json_paginated(&mut state, 5, 20, &bp)
        .await
        .unwrap();
    assert_eq!(
        ids(&page5),
        (81..=96).collect::<Vec<_>>(),
        "the chunk the repeated cursor points back to is read once; nothing past it"
    );
    assert_eq!(page5["scalars"]["has_next_page"], false);
    assert_eq!(state.io_count, 3, "the walk stopped at the repeat");
}
