//! Host-side fetching and extraction of dynamically populated extension filter options.

use crate::error::{Error, Result};
use crate::http::SmartClient;
use crate::wasm::AllowedHost;
use kani_shared::FilterFetchDef;

/// The host of a URL, or `None` if it has none / does not parse.
fn url_host(url: &str) -> Option<String> {
    url.parse::<url::Url>()
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
}

/// Resolve an option-set route against the source's `base_url`: an absolute
/// route is used as-is, a relative one is joined against the base (so
/// `/filters/genres` resolves the way the guest's own requests would). Without
/// this a relative route reached `SmartClient::get` with no base and failed.
fn resolve_route(base_url: &str, route: &str) -> Result<String> {
    if route.starts_with("http://") || route.starts_with("https://") {
        return Ok(route.to_string());
    }
    let base = url::Url::parse(base_url)
        .map_err(|e| Error::Other(format!("invalid source base_url '{base_url}': {e}")))?;
    base.join(route)
        .map(|u| u.to_string())
        .map_err(|e| Error::Other(format!("cannot resolve option-set route '{route}': {e}")))
}

/// Enforce the source's HTTP policy on a resolved option-set URL: unrestricted
/// sources may fetch anywhere, restricted ones only from their own host (exact
/// match, the same rule the guest HTTP path applies). Returns the policy so the
/// fetch can hold redirects to it too.
fn enforce_option_set_host(
    base_url: &str,
    url: &str,
    unrestricted_http: bool,
) -> Result<AllowedHost> {
    if unrestricted_http {
        return Ok(AllowedHost::Unrestricted);
    }
    let base_host = url_host(base_url)
        .ok_or_else(|| Error::Other(format!("source base_url '{base_url}' has no host")))?;
    let req_host = url_host(url)
        .ok_or_else(|| Error::Other(format!("option-set route '{url}' has no host")))?;
    let policy = AllowedHost::Restricted(base_host);
    policy.allows_host(&req_host).map_err(Error::Other)?;
    Ok(policy)
}

/// Fetches and parses a `FilterFetchDef`, returning `(name, value)` pairs.
/// Options with `nsfw=true` are excluded.
///
/// `base_url` / `unrestricted_http` are the owning source's HTTP policy: the
/// route is resolved against `base_url` and, unless the source is unrestricted,
/// must resolve to the source's own host — otherwise a def could point the
/// server at an arbitrary host, outside the WASM sandbox's allowed-host check.
pub async fn fetch_option_set(
    client: &SmartClient,
    def: &FilterFetchDef,
    base_url: &str,
    unrestricted_http: bool,
) -> Result<Vec<(String, String)>> {
    let url = resolve_route(base_url, &def.route)?;
    let policy = enforce_option_set_host(base_url, &url, unrestricted_http)?;

    // Bounded: this is an operator-supplied URL fetched to populate a filter
    // dropdown. An option set is kilobytes; anything past the cap is not a
    // document we were going to parse.
    let bytes = client
        .get_for_source(&url, policy)
        .await?
        .bytes_prefix(client.budgets().max_option_set_bytes)
        .await?;
    let response = String::from_utf8_lossy(&bytes).into_owned();

    match def.response_type.as_str() {
        "json" => parse_json_options(&response, def),
        _ => parse_html_options(&response, def),
    }
}

fn parse_html_options(html: &str, def: &FilterFetchDef) -> Result<Vec<(String, String)>> {
    use scraper::{Html, Selector};

    let doc = Html::parse_document(html);
    let container_sel = def.container.as_deref().unwrap_or(":root");
    let sel = Selector::parse(container_sel)
        .map_err(|_| Error::Internal(format!("invalid CSS selector: {container_sel}")))?;

    let name_expr = def.fields.get("name").map(String::as_str).unwrap_or("*");
    let value_expr = def.fields.get("value").map(String::as_str).unwrap_or("*");
    let nsfw_expr = def
        .nsfw_field
        .as_ref()
        .and_then(|f| def.fields.get(f.as_str()));

    let mut options = Vec::new();
    for element in doc.select(&sel) {
        let name = extract_html_value(element, name_expr);
        let value = extract_html_value(element, value_expr);
        if name.is_empty() || value.is_empty() {
            continue;
        }
        if let Some(nsfw_sel) = nsfw_expr {
            let nsfw_str = extract_html_value(element, nsfw_sel);
            if nsfw_str == "true" || nsfw_str == "1" {
                continue;
            }
        }
        options.push((name, value));
    }
    Ok(options)
}

/// Extracts a value from an HTML element. Format: `"sel"` (text), `"sel|attr"` (attribute),
/// `"self"` (element text), `"self|attr"` (element attribute).
fn extract_html_value(element: scraper::ElementRef<'_>, expr: &str) -> String {
    use scraper::Selector;

    if let Some((sel_part, attr)) = expr.split_once('|') {
        let target = if sel_part.is_empty() || sel_part == "self" {
            Some(element)
        } else {
            Selector::parse(sel_part)
                .ok()
                .and_then(|s| element.select(&s).next())
        };
        target
            .and_then(|el| el.attr(attr).map(str::to_owned))
            .unwrap_or_default()
    } else if expr.is_empty() || expr == "self" {
        element.text().collect::<String>().trim().to_owned()
    } else {
        match Selector::parse(expr) {
            Ok(s) => element
                .select(&s)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_owned())
                .unwrap_or_default(),
            Err(_) => String::new(),
        }
    }
}

fn parse_json_options(json: &str, def: &FilterFetchDef) -> Result<Vec<(String, String)>> {
    let root: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| Error::Internal(format!("JSON parse error: {e}")))?;

    let items = if let Some(container) = &def.container {
        root.pointer(container)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        root.as_array().cloned().unwrap_or_default()
    };

    let name_ptr = def
        .fields
        .get("name")
        .map(String::as_str)
        .unwrap_or("/name");
    let value_ptr = def
        .fields
        .get("value")
        .map(String::as_str)
        .unwrap_or("/value");
    let nsfw_ptr = def
        .nsfw_field
        .as_ref()
        .and_then(|f| def.fields.get(f.as_str()));

    let mut options = Vec::new();
    for item in &items {
        let name = item
            .pointer(name_ptr)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let value = item
            .pointer(value_ptr)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        if let Some(np) = nsfw_ptr {
            let nsfw = item.pointer(np).and_then(|v| v.as_bool()).unwrap_or(false);
            if nsfw {
                continue;
            }
        }
        options.push((name, value));
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn make_def(
        response_type: &str,
        container: Option<&str>,
        fields: Vec<(&str, &str)>,
        nsfw_field: Option<&str>,
    ) -> FilterFetchDef {
        FilterFetchDef {
            filter_id: "test".to_string(),
            option_set_name: "test_set".to_string(),
            route: "https://example.com".to_string(),
            response_type: response_type.to_string(),
            container: container.map(str::to_owned),
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            nsfw_field: nsfw_field.map(str::to_owned),
            cache_key: None,
            cache_ttl: 300,
        }
    }

    #[test]
    fn html_text_extraction() {
        let html = r#"<ul><li><span class="name">Action</span><a data-id="action">action</a></li><li><span class="name">Comedy</span><a data-id="comedy">comedy</a></li></ul>"#;
        let def = make_def(
            "html",
            Some("li"),
            vec![("name", "span.name"), ("value", "a|data-id")],
            None,
        );
        let opts = parse_html_options(html, &def).unwrap();
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0], ("Action".to_string(), "action".to_string()));
        assert_eq!(opts[1], ("Comedy".to_string(), "comedy".to_string()));
    }

    #[test]
    fn html_nsfw_filtering() {
        let html_full = r#"<ul>
            <li><span class="name">Action</span><span class="nsfw">false</span><a data-id="1">1</a></li>
            <li><span class="name">Adult</span><span class="nsfw">true</span><a data-id="2">2</a></li>
        </ul>"#;
        let def = make_def(
            "html",
            Some("li"),
            vec![
                ("name", "span.name"),
                ("value", "a|data-id"),
                ("nsfw", "span.nsfw"),
            ],
            Some("nsfw"),
        );
        let opts = parse_html_options(html_full, &def).unwrap();
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].0, "Action");
    }

    #[test]
    fn json_extraction() {
        let json = r#"[{"name":"Action","id":"action"},{"name":"Comedy","id":"comedy"}]"#;
        let def = make_def(
            "json",
            None,
            vec![("name", "/name"), ("value", "/id")],
            None,
        );
        let opts = parse_json_options(json, &def).unwrap();
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0], ("Action".to_string(), "action".to_string()));
    }

    #[test]
    fn json_container_path() {
        let json = r#"{"tags":[{"name":"Action","id":"action"}]}"#;
        let def = make_def(
            "json",
            Some("/tags"),
            vec![("name", "/name"), ("value", "/id")],
            None,
        );
        let opts = parse_json_options(json, &def).unwrap();
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].0, "Action");
    }

    #[test]
    fn resolve_route_passes_through_absolute() {
        assert_eq!(
            resolve_route("https://src.example", "https://cdn.example/x").unwrap(),
            "https://cdn.example/x"
        );
    }

    #[test]
    fn resolve_route_joins_relative_against_base() {
        assert_eq!(
            resolve_route("https://src.example/manga/", "/api/genres").unwrap(),
            "https://src.example/api/genres"
        );
    }

    #[test]
    fn restricted_source_blocks_a_host_escape() {
        let err = enforce_option_set_host(
            "https://source.invalid",
            "https://evil.invalid/steal",
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("may only contact"),
            "expected an allowed-host refusal, got: {err}"
        );
    }

    #[test]
    fn restricted_source_allows_its_own_host() {
        let url = resolve_route("https://source.invalid/manga/", "/api/genres").unwrap();
        assert!(enforce_option_set_host("https://source.invalid", &url, false).is_ok());
    }

    #[test]
    fn unrestricted_source_allows_any_host() {
        assert!(
            enforce_option_set_host("https://source.invalid", "https://anywhere.invalid/x", true)
                .is_ok()
        );
    }

    /// The option-set ceiling truncates rather than refusing, so an oversized
    /// document yields the options that fit and drops the rest. Proving that
    /// needs the budget seam: the shipped ceiling is 4 MB.
    #[tokio::test]
    async fn an_option_set_past_the_budget_is_truncated_at_the_ceiling() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let mut body = String::from("<html><body><ul>");
        for i in 0..200 {
            body.push_str(&format!(
                "<li><span class=\"name\">Option {i}</span><a data-id=\"v{i}\">v{i}</a></li>"
            ));
        }
        body.push_str("</ul></body></html>");
        let full_len = body.len();

        Mock::given(method("GET"))
            .and(path("/options"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body.into_bytes(), "text/html"))
            .mount(&server)
            .await;

        let def = FilterFetchDef {
            route: format!("{}/options", server.uri()),
            ..make_def(
                "html",
                Some("li"),
                vec![("name", "span.name"), ("value", "a|data-id")],
                None,
            )
        };

        let generous = SmartClient::new_for_test()
            .unwrap()
            .with_allow_loopback_egress(true);
        let all = fetch_option_set(&generous, &def, &server.uri(), true)
            .await
            .unwrap();
        assert!(all.len() > 10, "fixture must produce a long option set");

        let stingy = SmartClient::new_for_test()
            .unwrap()
            .with_allow_loopback_egress(true)
            .with_budgets(crate::http::Budgets {
                max_option_set_bytes: full_len / 8,
                ..crate::http::Budgets::default()
            });
        let truncated = fetch_option_set(&stingy, &def, &server.uri(), true)
            .await
            .unwrap();

        assert!(
            truncated.len() < all.len(),
            "the ceiling did not bound the document: {} vs {}",
            truncated.len(),
            all.len()
        );
        assert!(
            !truncated.is_empty(),
            "truncation must keep the prefix, not discard everything"
        );
    }

    #[test]
    fn json_nsfw_filtered() {
        let json =
            r#"[{"name":"Safe","id":"s","adult":false},{"name":"NSFW","id":"n","adult":true}]"#;
        let def = make_def(
            "json",
            None,
            vec![("name", "/name"), ("value", "/id"), ("nsfw", "/adult")],
            Some("nsfw"),
        );
        let opts = parse_json_options(json, &def).unwrap();
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].0, "Safe");
    }
}
