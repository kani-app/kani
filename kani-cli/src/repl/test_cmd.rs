use crate::error::CliError;
use crate::repl::har;
use crate::yaml::{
    model::{ValidatedEndpoint, ValidatedExtension, ValidatedPopular},
    schema::{ResponseType, YamlExtension},
    validate,
};
use std::path::Path;

pub fn run_test(
    file: &str,
    har_path: &str,
    endpoint: &str,
    expected_count: usize,
    url_contains: Option<&str>,
) -> Result<(), CliError> {
    let actual = evaluate_endpoint_count(file, har_path, endpoint, url_contains)?;
    if actual == expected_count {
        println!("ok — {actual} row(s)");
        Ok(())
    } else {
        Err(CliError::Other(format!(
            "row count mismatch: expected {expected_count}, got {actual}"
        )))
    }
}

pub fn run_replay(
    file: &str,
    har_path: &str,
    endpoint: &str,
    expected_path: &str,
    url_contains: Option<&str>,
) -> Result<(), CliError> {
    let (ext, ep) = load_endpoint(file, endpoint)?;
    let har = har::load(har_path)?;
    let body = find_response_body(&har, &ep, url_contains)?;
    let actual = extract_rows(&ext, &ep, endpoint, &body)?;

    let expected_src = std::fs::read_to_string(expected_path)?;
    let expected: serde_json::Value = serde_json::from_str(&expected_src)
        .map_err(|e| CliError::Other(format!("expected JSON parse error: {e}")))?;

    if actual == expected {
        println!("ok — replay matches expected output");
        Ok(())
    } else {
        let actual_pretty =
            serde_json::to_string_pretty(&actual).unwrap_or_else(|_| format!("{actual:?}"));
        let expected_pretty =
            serde_json::to_string_pretty(&expected).unwrap_or_else(|_| format!("{expected:?}"));
        eprintln!("--- expected ---");
        eprintln!("{expected_pretty}");
        eprintln!("--- actual ---");
        eprintln!("{actual_pretty}");
        Err(CliError::Other(
            "replay output does not match expected".into(),
        ))
    }
}

fn evaluate_endpoint_count(
    file: &str,
    har_path: &str,
    endpoint: &str,
    url_contains: Option<&str>,
) -> Result<usize, CliError> {
    let (ext, ep) = load_endpoint(file, endpoint)?;
    let har = har::load(har_path)?;
    let body = find_response_body(&har, &ep, url_contains)?;
    let rows = extract_rows(&ext, &ep, endpoint, &body)?;
    match rows {
        serde_json::Value::Array(arr) => Ok(arr.len()),
        _ => Ok(1),
    }
}

pub fn load_endpoint(
    file: &str,
    endpoint: &str,
) -> Result<(ValidatedExtension, ValidatedEndpoint), CliError> {
    let path = Path::new(file);
    let src = std::fs::read_to_string(path)?;
    let ext: YamlExtension = serde_yaml::from_str(&src)
        .map_err(|e| CliError::Other(format!("YAML parse error: {e}")))?;
    let mut validated = validate::validate(&ext, &src, path).map_err(|errors| {
        for e in &errors {
            eprintln!("  {e}");
        }
        CliError::Other(format!("{} validation error(s)", errors.len()))
    })?;

    let selected = match endpoint {
        "popular" => match validated.popular.take() {
            Some(ValidatedPopular::Full(ep)) => Ok(*ep),
            Some(ValidatedPopular::Delegated { delegate_to, .. }) => Err(CliError::Other(format!(
                "popular endpoint delegates to {delegate_to}, not a direct fetch"
            ))),
            None => Err(CliError::Other("no popular endpoint defined".into())),
        },
        "search" => validated
            .search
            .take()
            .ok_or_else(|| CliError::Other("no search endpoint defined".into())),
        "manga_details" | "details" => validated
            .manga_details
            .take()
            .ok_or_else(|| CliError::Other("no manga_details endpoint defined".into())),
        "chapter_list" | "chapters" => validated
            .chapter_list
            .take()
            .ok_or_else(|| CliError::Other("no chapter_list endpoint defined".into())),
        "pages" => validated
            .pages
            .take()
            .ok_or_else(|| CliError::Other("no pages endpoint defined".into())),
        other => Err(CliError::Other(format!("unknown endpoint: {other}"))),
    }?;

    Ok((validated, selected))
}

/// The literal parts of a route, in order, with `$placeholder$` spans removed.
/// `/manga/$manga_id$/chapters` yields `["/manga/", "/chapters"]`.
fn route_literals(route: &str) -> Vec<&str> {
    let mut literals = Vec::new();
    let mut rest = route;
    while let Some(open) = rest.find('$') {
        if !rest[..open].is_empty() {
            literals.push(&rest[..open]);
        }
        let Some(close) = rest[open + 1..].find('$') else {
            return literals;
        };
        rest = &rest[open + 1 + close + 1..];
    }
    if !rest.is_empty() {
        literals.push(rest);
    }
    literals
}

/// Whether `url` contains every literal of the route, in order.
fn matches_route(url: &str, route: &str) -> bool {
    let mut cursor = 0;
    for literal in route_literals(route) {
        match url[cursor..].find(literal) {
            Some(at) => cursor += at + literal.len(),
            None => return false,
        }
    }
    true
}

/// Picks the HAR entry belonging to this endpoint.
///
/// A HAR recorded by `kani-cli repl record` holds one entry, but one exported from
/// a browser holds every request the page made — where the first successful
/// response is usually a stylesheet. Matching on the route rather than taking the
/// first success is what tells those apart, and no match is an error rather than a
/// guess.
fn find_response_body(
    har: &har::Har,
    ep: &ValidatedEndpoint,
    url_contains: Option<&str>,
) -> Result<String, CliError> {
    let entry = match url_contains {
        Some(fragment) => har::find_entry(har, fragment).ok_or_else(|| {
            CliError::Other(format!(
                "no HAR entry URL contains {fragment:?} ({} entries searched)",
                har.log.entries.len()
            ))
        })?,
        None => har
            .log
            .entries
            .iter()
            .find(|e| e.response.status < 400 && matches_route(&e.request.url, &ep.route))
            .ok_or_else(|| {
                CliError::Other(format!(
                    "no successful HAR entry matches route {:?} ({} entries searched); \
                     pass --url-contains <fragment> to name the entry directly",
                    ep.route,
                    har.log.entries.len()
                ))
            })?,
    };

    entry.response.content.text.clone().ok_or_else(|| {
        CliError::Other(format!(
            "HAR entry {} has no response body",
            entry.request.url
        ))
    })
}

/// A host state wired with the extension's hooks and scripts, so a fixture run
/// exercises the same request and extraction path the server would.
pub(crate) fn host_state_for(
    ext: &ValidatedExtension,
) -> Result<kani_core::wasm::HostState, CliError> {
    let solver = std::env::var("KANI_SOLVER_URL")
        .ok()
        .filter(|v| !v.is_empty());
    let http = kani_core::http::SmartClient::new(solver)
        .map_err(|e| CliError::Other(format!("http client: {e}")))?;
    let mut state = kani_core::wasm::HostState::new(
        http,
        kani_core::wasm::AllowedHost::Unrestricted,
        std::sync::Arc::new(kani_core::cache::InMemoryCache::new()),
        format!("{}:", ext.id),
        kani_core::v8_process::new_handle(),
    )
    .map_err(|e| CliError::Other(format!("host state: {e}")))?;

    let hooks = kani_core::scripting::HookScripts {
        shared: ext.pure_scripts.clone(),
        pre_request: ext.pre_request.clone(),
        on_status: ext.on_status.clone(),
        endpoint_pre_request: ext.endpoint_pre_request.clone(),
        endpoint_on_status: ext.endpoint_on_status.clone(),
        cache: ext.cache_limits(),
    };
    if !hooks.is_empty() {
        state.hook_registry = Some(std::sync::Arc::new(
            kani_core::scripting::HookRegistry::compile(&hooks).map_err(CliError::Other)?,
        ));
    }
    if !ext.pure_scripts.is_empty() {
        state.pure_fn_registry = Some(std::sync::Arc::new(
            kani_core::scripting::PureFunctionRegistry::compile(&ext.pure_scripts)
                .map_err(CliError::Other)?,
        ));
    }
    state.browser_scripts = Some(std::sync::Arc::new(
        kani_core::scripting::BrowserScriptRegistry::from_map(&ext.browser_scripts),
    ));

    Ok(state)
}

fn extract_rows(
    ext: &ValidatedExtension,
    ep: &ValidatedEndpoint,
    endpoint_name: &str,
    body: &str,
) -> Result<serde_json::Value, CliError> {
    use kani_core::evaluator::{html_eval, json_eval};

    let blueprint = kani_yaml::build_blueprint_core(ep, ext, endpoint_name).build();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::Other(format!("runtime: {e}")))?;

    let output = runtime.block_on(async {
        let mut state = host_state_for(ext)?;

        match ep.response_type {
            ResponseType::Json => json_eval::extract_json_str(&mut state, body, &blueprint).await,
            ResponseType::Html => html_eval::extract_html_str(&mut state, body, &blueprint).await,
        }
        .map_err(CliError::Other)
    })?;

    Ok(output["rows"].clone())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn entry(url: &str, status: u16, body: &str) -> har::HarEntry {
        har::HarEntry {
            request: har::HarRequest {
                method: "GET".into(),
                url: url.into(),
            },
            response: har::HarResponse {
                status,
                content: har::HarContent {
                    mime_type: "application/json".into(),
                    text: Some(body.into()),
                },
            },
        }
    }

    fn har_of(entries: Vec<har::HarEntry>) -> har::Har {
        har::Har {
            log: har::HarLog { entries },
        }
    }

    /// A browser export: the first successful response is a stylesheet, and the
    /// endpoint's own response is buried among the page's other requests.
    fn browser_har() -> har::Har {
        har_of(vec![
            entry("https://ex.com/assets/app.css", 200, "body{}"),
            entry("https://ex.com/manga/8813", 200, r#"{"title":"wrong"}"#),
            entry("https://ex.com/manga/8813/chapters", 200, r#"["right"]"#),
        ])
    }

    /// A real validated endpoint with its route swapped, so matching is exercised
    /// against the same type the commands pass in.
    fn route_ep(route: &str) -> ValidatedEndpoint {
        let (_ext, mut ep) =
            load_endpoint("tests/fixtures/chapter_list.yaml", "chapter_list").unwrap();
        ep.route = route.to_string();
        ep
    }

    #[test]
    fn route_literals_drop_the_placeholder_spans() {
        assert_eq!(
            route_literals("/manga/$manga_id$/chapters"),
            vec!["/manga/", "/chapters"]
        );
        assert_eq!(route_literals("/popular"), vec!["/popular"]);
        assert_eq!(route_literals("$id$"), Vec::<&str>::new());
    }

    #[test]
    fn a_route_matches_only_when_every_literal_appears_in_order() {
        assert!(matches_route(
            "https://ex.com/manga/8813/chapters",
            "/manga/$id$/chapters"
        ));
        // Present but reversed: the trailing literal must follow the leading one.
        assert!(!matches_route(
            "https://ex.com/chapters/8813/manga/",
            "/manga/$id$/chapters"
        ));
        assert!(!matches_route(
            "https://ex.com/manga/8813",
            "/manga/$id$/chapters"
        ));
        assert!(!matches_route("https://ex.com/assets/app.css", "/popular"));
    }

    #[test]
    fn the_entry_matching_the_route_is_chosen_over_earlier_successes() {
        let body = find_response_body(
            &browser_har(),
            &route_ep("/manga/$manga_id$/chapters"),
            None,
        )
        .unwrap();

        assert_eq!(
            body, r#"["right"]"#,
            "the stylesheet and the details response both precede the chapter list"
        );
    }

    #[test]
    fn no_matching_entry_is_an_error_naming_the_route() {
        let error = find_response_body(&browser_har(), &route_ep("/search"), None).unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("/search") && message.contains("3 entries"),
            "the error must name the route and how many entries were searched, got: {message}"
        );
        assert!(
            message.contains("--url-contains"),
            "the error must point at the escape hatch, got: {message}"
        );
    }

    #[test]
    fn an_explicit_fragment_overrides_route_matching() {
        let body = find_response_body(
            &browser_har(),
            &route_ep("/search"),
            Some("/manga/8813/chapters"),
        )
        .unwrap();

        assert_eq!(body, r#"["right"]"#);
    }

    #[test]
    fn a_single_entry_recording_still_matches_its_own_route() {
        let har = har_of(vec![entry(
            "https://ex.com/manga/8813/chapters?page=1",
            200,
            r#"["ok"]"#,
        )]);

        let body = find_response_body(&har, &route_ep("/manga/$manga_id$/chapters"), None).unwrap();

        assert_eq!(
            body, r#"["ok"]"#,
            "record writes base_url + route, so a recorded HAR always matches"
        );
    }

    #[test]
    fn a_failed_response_is_not_replayed() {
        let har = har_of(vec![entry("https://ex.com/manga/1/chapters", 503, "nope")]);

        let error =
            find_response_body(&har, &route_ep("/manga/$manga_id$/chapters"), None).unwrap_err();

        assert!(error.to_string().contains("no successful HAR entry"));
    }
}
