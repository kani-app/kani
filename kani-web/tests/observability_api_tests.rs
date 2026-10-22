#![allow(clippy::unwrap_used)]

mod common;
use common::{build_test_app, get_req, test_state};
use tower::ServiceExt;

#[tokio::test]
async fn every_response_carries_an_x_request_id_header() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app.oneshot(get_req("/rest/sources")).await.unwrap();

    let id = res
        .headers()
        .get("x-request-id")
        .expect("every response must carry x-request-id")
        .to_str()
        .unwrap();
    assert_eq!(id.len(), 36, "expected a hyphenated uuid v4, got {id}");
}

#[tokio::test]
async fn error_responses_also_carry_an_x_request_id_header() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app.oneshot(get_req("/rest/sources")).await.unwrap();

    assert!(
        res.status().is_client_error(),
        "unauthenticated request should be a 4xx, got {}",
        res.status()
    );
    assert!(
        res.headers().contains_key("x-request-id"),
        "error responses must still carry a trace id"
    );
}

#[tokio::test]
async fn inbound_x_request_id_is_echoed_back() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let supplied = "11111111-2222-3333-4444-555555555555";
    let req = axum::http::Request::builder()
        .uri("/rest/sources")
        .header("x-request-id", supplied)
        .body(axum::body::Body::empty())
        .unwrap();

    let res = app.oneshot(req).await.unwrap();

    assert_eq!(
        res.headers().get("x-request-id").unwrap().to_str().unwrap(),
        supplied,
        "a caller-supplied trace id must be propagated, not replaced"
    );
}

#[tokio::test]
async fn metrics_endpoint_is_denied_without_a_credential() {
    kani_web::metrics::describe();
    let app = kani_web::metrics::router(test_state().await);

    let res = app.oneshot(get_req("/metrics")).await.unwrap();

    assert_eq!(
        res.status(),
        axum::http::StatusCode::UNAUTHORIZED,
        "metrics disclose extension and upstream host names; they must not be \
         readable without a credential"
    );
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        text.contains("metrics:read"),
        "the refusal should say how to enable scraping, got: {text}"
    );
}

#[tokio::test]
async fn a_metrics_scoped_api_token_can_scrape() {
    use kani_app::service::api_tokens::TokenKind;

    let state = test_state().await;
    common::create_admin(&state).await;
    let uid: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'admin'")
        .fetch_one(&state.db)
        .await
        .unwrap();
    let scope: kani_app::permissions::Permission = "metrics:read".parse().unwrap();
    let created = state
        .service
        .create_token(
            kani_app::ids::UserId(uid),
            "scraper",
            None,
            TokenKind::Api,
            Some(&[scope]),
        )
        .await
        .unwrap();

    kani_web::metrics::describe();
    let app = kani_web::metrics::router(state);
    let res = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/metrics")
                .header(
                    axum::http::header::AUTHORIZATION,
                    format!("Bearer {}", created.raw_token),
                )
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        axum::http::StatusCode::OK,
        "a revocable per-scraper credential must work without the shared env token"
    );
}

#[tokio::test]
async fn registered_kani_metrics_are_present_in_the_exposition() {
    kani_web::metrics::describe();
    let handle = &kani_web::metrics::prometheus().1;
    let text = handle.render();
    for metric in [
        "kani_log_errors_total",
        "kani_sse_clients",
        "kani_jobs_running",
    ] {
        assert!(
            text.contains(metric),
            "{metric} should be pre-registered so it is scrapeable before first use"
        );
    }
    assert!(
        text.contains("# TYPE"),
        "expected prometheus exposition format"
    );
}

#[tokio::test]
async fn each_request_gets_a_distinct_generated_id() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let first = app.clone().oneshot(get_req("/rest/sources")).await.unwrap();
    let second = app.oneshot(get_req("/rest/sources")).await.unwrap();

    let a = first.headers().get("x-request-id").unwrap();
    let b = second.headers().get("x-request-id").unwrap();
    assert_ne!(a, b, "generated trace ids must be unique per request");
}

#[tokio::test]
async fn diagnostics_returns_payload_for_admin() {
    let state = test_state().await;
    let (username, password) = common::create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    let res = app
        .oneshot(common::authed_get("/rest/admin/diagnostics", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let body = common::body_json(res).await;
    assert!(body["version"].is_string(), "version missing: {body}");
    assert!(body["uptime_secs"].is_number());
    assert!(body["extensions"].is_array());
    assert!(
        body["browser"]["calls_total"].is_number(),
        "browser section (plan 02 stats) missing: {body}"
    );
    assert!(
        body["browser"]["solver"].is_string(),
        "the browser section reports solver capability, not a local browser: {body}"
    );
    assert!(body["browser"]["solver_attempts"].is_number());
    assert!(body["browser"]["solver_successes"].is_number());
    assert!(body["browser"]["solver_failures"].is_number());
    assert!(body["browser"]["graceful_shutdowns"].is_number());
    assert!(body["browser"]["forced_terminations"].is_number());
}

#[tokio::test]
async fn support_bundle_returns_a_zip_with_expected_entries() {
    let state = test_state().await;
    let (username, password) = common::create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    let res = app
        .oneshot(common::authed_get("/rest/admin/support-bundle", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let disposition = res
        .headers()
        .get(axum::http::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        disposition.contains("kani-support-") && disposition.contains(".zip"),
        "unexpected content-disposition: {disposition}"
    );

    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();

    for expected in [
        "kani_info.json",
        "config.json",
        "db_schema.sql",
        "extensions.json",
        "diagnostics.json",
        "logs.jsonl",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "missing {expected} in {names:?}"
        );
    }

    let mut info = String::new();
    {
        use std::io::Read;
        zip.by_name("kani_info.json")
            .unwrap()
            .read_to_string(&mut info)
            .unwrap();
    }
    let info: serde_json::Value = serde_json::from_str(&info).unwrap();
    let schema_version = info["db_schema_version"].as_i64();
    assert!(
        schema_version.is_some_and(|v| v > 0),
        "kani_info.json must report the applied migration version so a bug report says which \
         schema produced it, got {:?}",
        info["db_schema_version"]
    );
}

#[tokio::test]
async fn system_update_reports_current_version_for_authed_user() {
    let state = test_state().await;
    let (username, password) = common::create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    let res = app
        .oneshot(common::authed_get("/rest/system/update", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let body = common::body_json(res).await;
    assert_eq!(
        body["current"].as_str().unwrap(),
        env!("CARGO_PKG_VERSION"),
        "should report the running version"
    );
    assert_eq!(
        body["update_available"], false,
        "no check has run, so no update should be claimed"
    );
}

#[tokio::test]
async fn system_update_requires_authentication() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app.oneshot(get_req("/rest/system/update")).await.unwrap();

    assert_eq!(res.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[derive(Debug)]
struct Exposition {
    /// metric base name → declared type
    declared: std::collections::HashMap<String, String>,
    helped: std::collections::HashSet<String>,
    /// (full sample name, label block, value)
    samples: Vec<(String, Option<String>, String)>,
}

fn is_valid_metric_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == ':' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

/// Parse the text exposition format strictly: anything we cannot account for is
/// a failure, because a scraper would reject it too.
fn parse_exposition(body: &str) -> Exposition {
    let mut out = Exposition {
        declared: std::collections::HashMap::new(),
        helped: std::collections::HashSet::new(),
        samples: Vec::new(),
    };

    for (lineno, raw) in body.lines().enumerate() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# TYPE ") {
            let mut parts = rest.split_whitespace();
            let name = parts.next().unwrap_or_default().to_string();
            let kind = parts.next().unwrap_or_default().to_string();
            assert!(
                !name.is_empty() && !kind.is_empty(),
                "line {}: malformed TYPE line: {line}",
                lineno + 1
            );
            assert!(
                out.declared.insert(name.clone(), kind).is_none(),
                "line {}: duplicate TYPE declaration for {name}",
                lineno + 1
            );
            continue;
        }
        if let Some(rest) = line.strip_prefix("# HELP ") {
            let name = rest.split_whitespace().next().unwrap_or_default();
            assert!(
                !name.is_empty(),
                "line {}: malformed HELP line: {line}",
                lineno + 1
            );
            out.helped.insert(name.to_string());
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let (name_and_labels, value) = line
            .rsplit_once(' ')
            .unwrap_or_else(|| panic!("line {}: sample has no value: {line}", lineno + 1));
        let (name, labels) = match name_and_labels.split_once('{') {
            Some((n, l)) => {
                let l = l.strip_suffix('}').unwrap_or_else(|| {
                    panic!("line {}: unterminated label block: {line}", lineno + 1)
                });
                (n.to_string(), Some(l.to_string()))
            }
            None => (name_and_labels.to_string(), None),
        };
        assert!(
            value.parse::<f64>().is_ok()
                || value == "NaN"
                || value.starts_with("+Inf")
                || value.starts_with("-Inf"),
            "line {}: sample value is not a number: {line}",
            lineno + 1
        );
        out.samples.push((name, labels, value.to_string()));
    }
    out
}

/// The base metric a sample belongs to, stripping the suffixes histograms and
/// summaries add to their derived series.
fn base_name(sample: &str) -> String {
    for suffix in ["_bucket", "_sum", "_count", "_total"] {
        if let Some(stripped) = sample.strip_suffix(suffix) {
            return stripped.to_string();
        }
    }
    sample.to_string()
}

#[tokio::test]
async fn the_metrics_exposition_parses_as_prometheus_text_format() {
    kani_web::metrics::describe();
    let text = kani_web::metrics::prometheus().1.render();
    let parsed = parse_exposition(&text);
    assert!(
        !parsed.declared.is_empty(),
        "an exposition with no declared metrics is not worth scraping"
    );
}

#[tokio::test]
async fn every_metric_has_a_matching_help_and_type_line() {
    kani_web::metrics::describe();
    let text = kani_web::metrics::prometheus().1.render();
    let parsed = parse_exposition(&text);

    for (sample, _, _) in &parsed.samples {
        let base = base_name(sample);
        assert!(
            parsed.declared.contains_key(&base) || parsed.declared.contains_key(sample),
            "sample {sample} has no # TYPE declaration; a scraper treats it as untyped"
        );
    }
    for name in parsed.declared.keys() {
        assert!(
            parsed.helped.contains(name),
            "{name} is TYPEd but has no # HELP line"
        );
    }
}

#[tokio::test]
async fn metric_names_are_valid_identifiers() {
    kani_web::metrics::describe();
    let text = kani_web::metrics::prometheus().1.render();
    let parsed = parse_exposition(&text);

    for name in parsed.declared.keys() {
        assert!(
            is_valid_metric_name(name),
            "{name} is not a valid Prometheus metric name"
        );
    }
    for (sample, labels, _) in &parsed.samples {
        assert!(
            is_valid_metric_name(sample),
            "{sample} is not a valid Prometheus metric name"
        );
        if let Some(l) = labels {
            assert!(
                !l.contains('\n'),
                "{sample} has a raw newline in its label block"
            );
            for pair in l.split("\",") {
                if pair.trim().is_empty() {
                    continue;
                }
                assert!(
                    pair.contains("=\""),
                    "{sample} has an unquoted label value: {pair}"
                );
            }
        }
    }
}

#[tokio::test]
async fn counters_do_not_go_backwards_across_two_scrapes() {
    kani_web::metrics::describe();
    let handle = &kani_web::metrics::prometheus().1;

    let first = parse_exposition(&handle.render());
    let second = parse_exposition(&handle.render());

    let counters: std::collections::HashSet<&String> = first
        .declared
        .iter()
        .filter(|(_, kind)| kind.as_str() == "counter")
        .map(|(name, _)| name)
        .collect();

    for (sample, labels, value) in &second.samples {
        if !counters.contains(&base_name(sample)) {
            continue;
        }
        let before = first
            .samples
            .iter()
            .find(|(n, l, _)| n == sample && l == labels)
            .and_then(|(_, _, v)| v.parse::<f64>().ok());
        let after = value.parse::<f64>().unwrap_or(f64::NAN);
        if let Some(before) = before {
            assert!(
                after >= before,
                "counter {sample} went backwards: {before} → {after}"
            );
        }
    }
}

#[test]
#[should_panic(expected = "sample value is not a number")]
fn the_exposition_parser_rejects_a_non_numeric_sample() {
    parse_exposition("# HELP x h\n# TYPE x counter\nx not_a_number\n");
}

#[test]
#[should_panic(expected = "duplicate TYPE declaration")]
fn the_exposition_parser_rejects_a_duplicate_type_declaration() {
    parse_exposition("# TYPE x counter\n# TYPE x gauge\n");
}

#[test]
#[should_panic(expected = "unterminated label block")]
fn the_exposition_parser_rejects_an_unterminated_label_block() {
    parse_exposition("# HELP x h\n# TYPE x counter\nx{a=\"1\" 5\n");
}

#[test]
fn a_sample_without_a_type_declaration_is_detected() {
    let parsed = parse_exposition("# HELP x h\n# TYPE x counter\nx 1\ny 2\n");
    assert!(parsed.declared.contains_key("x"));
    assert!(
        !parsed.declared.contains_key("y"),
        "y is undeclared, which is exactly what R2 fails on"
    );
}
