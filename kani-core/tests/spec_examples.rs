#![allow(clippy::unwrap_used)]

//! Every DSL expression shown in `SPECIFICATION.md` must parse, lower, and evaluate without error.
//! Containers are replaced by the fixture root: this checks the expressions, not the selectors.

use kani_core::evaluator::{html_eval, json_eval};
use kani_core::wasm::HostState;
use kani_shared::ast::{BlueprintBuilder, Expr};
use serde_yaml::Value as Yaml;

const SPEC: &str = include_str!("../../SPECIFICATION.md");
const SCHEMA_MARKER: &str = "<!-- schema sketch: not an executable example -->";

const HTML_FIXTURE: &str = r#"<html><head>
<meta property="og:url" content="https://example.com/manga/abc">
</head><body>
<h1 class="font-bold">Title</h1>
<p class="text-sm">Description</p>
<p class="description">Description</p>
<div class="status">Publishing</div>
<span class="status">Adult</span>
<span class="is-completed">Completed</span>
<span class="count">12</span>
<input id="search" value="one piece">
<input class="required" value="x">
<img class="cover" src="/cover.jpg" data-src="/cover.jpg">
<img class="js-page" data-src="/page1.jpg">
<a class="line-clamp-1" href="/manga/abc/def">Name</a>
<div class="line-clamp-2">Name</div>
<div class="col-span-2">Next</div>
<div class="id">abc</div>
<div class="title">Title</div>
<a class="link" href="/manga/abc">Link</a>
<a class="banner-link" href="/banner">Banner</a>
<a class="tag" href="/t/1">Action</a>
<a class="genre" href="/g/1">Action</a>
<a class="category" href="/c/1">Drama</a>
<a class="chapter" href="/c/2">Chapter 2</a>
<a class="chapter" href="/c/1">Chapter 1</a>
<div class="mb-3"><a class="text-sm" href="/t/2">Romance</a></div>
<table><tr><td class="price">1.5</td><td class="price">2</td></tr></table>
<ul><li><a href="/1">One</a></li><li><a href="/2">Two</a></li><li><span>Three</span></li></ul>
<div class="grid"><div></div><div><div></div><div>ongoing</div></div><div><div></div><div><div></div><div>Publishing</div></div></div></div>
</body></html>"#;

fn json_fixture() -> serde_json::Value {
    let attributes = serde_json::json!({
        "title": { "en": "Title", "ja": "タイトル" },
        "altTitles": [{ "en": "Alt" }, { "ja": "別" }],
        "fileName": "cover.jpg"
    });
    serde_json::json!({
        "id": "abc",
        "title": "Title",
        "status": "ongoing",
        "manga_id": "abc",
        "filename": "cover.jpg",
        "offset": 0,
        "limit": 20,
        "total": 100,
        "attributes": attributes,
        "relationships": [{ "type": "cover_art", "attributes": attributes }],
        "details": { "canonical_id": "abc" },
        "data": {
            "attributes": attributes,
            "relationships": [{ "type": "cover_art", "attributes": attributes }]
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Html,
    Json,
}

struct Example {
    origin: String,
    source: String,
    mode: Mode,
}

fn fenced_blocks() -> Vec<(usize, String, String)> {
    let mut blocks = Vec::new();
    let mut previous = "";
    let mut open: Option<(usize, String, Vec<&str>)> = None;
    for (index, line) in SPEC.lines().enumerate() {
        match open.as_mut() {
            Some((_, _, body)) if line != "```" => body.push(line),
            Some(_) => {
                let (start, info, body) = open.take().unwrap();
                blocks.push((start, info, body.join("\n")));
            }
            None => {
                if let Some(info) = line.strip_prefix("```") {
                    if previous.trim() != SCHEMA_MARKER {
                        open = Some((index + 1, info.trim().to_owned(), Vec::new()));
                    } else {
                        open = Some((0, "skip".into(), Vec::new()));
                    }
                }
                if !line.trim().is_empty() {
                    previous = line;
                }
            }
        }
    }
    blocks.retain(|(start, _, _)| *start != 0);
    blocks
}

fn mode_of_bare(source: &str) -> Mode {
    if source.contains("json(") || source.contains(".ptr(") {
        Mode::Json
    } else {
        Mode::Html
    }
}

fn field_exprs(value: &Yaml) -> Vec<(String, &str)> {
    let Some(map) = value.as_mapping() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, def) in map {
        let name = name.as_str().unwrap_or("?").to_owned();
        match def {
            Yaml::String(s) => out.push((name, s.as_str())),
            Yaml::Mapping(inner) => match inner.get("expr") {
                Some(Yaml::String(s)) => out.push((name, s.as_str())),
                _ => {
                    for (sub, sub_def) in inner {
                        if let Some(s) = sub_def.as_str() {
                            let sub = sub.as_str().unwrap_or("?");
                            out.push((format!("{name}.{sub}"), s));
                        }
                    }
                }
            },
            _ => {}
        }
    }
    out
}

fn walk_yaml(value: &Yaml, mode: Mode, path: &str, out: &mut Vec<Example>) {
    match value {
        Yaml::Mapping(map) => {
            let is_json = map.get("type").and_then(Yaml::as_str) == Some("json")
                || map.get("via").and_then(Yaml::as_str) == Some("browser_payload");
            let mode = if is_json { Mode::Json } else { mode };
            for (key, child) in map {
                let key = key.as_str().unwrap_or("?");
                let here = format!("{path}.{key}");
                let mut push = |origin: String, source: &str, mode: Mode| {
                    out.push(Example {
                        origin,
                        source: source.to_owned(),
                        mode,
                    })
                };
                match key {
                    "options_fetched_by" => {}
                    "fields" | "scalars" | "bindings" => {
                        for (name, source) in field_exprs(child) {
                            push(format!("{here}.{name}"), source, mode);
                        }
                    }
                    "url_expr" | "has_next_page" | "total_pages" => {
                        if let Some(source) = child.as_str() {
                            push(here, source, mode);
                        }
                    }
                    "deduplicate_by" => {
                        if let Some(source) = child.as_str() {
                            push(here, source, Mode::Json);
                        }
                    }
                    "on_failure" => {
                        if let Some(source) = child.as_str()
                            && source != "skip"
                            && source != "fail"
                        {
                            push(here, source, mode);
                        }
                    }
                    _ => walk_yaml(child, mode, &here, out),
                }
            }
        }
        Yaml::Sequence(items) => {
            for (index, item) in items.iter().enumerate() {
                walk_yaml(item, mode, &format!("{path}[{index}]"), out);
            }
        }
        _ => {}
    }
}

fn collect_examples() -> (Vec<Example>, Vec<String>) {
    let mut examples = Vec::new();
    let mut failures = Vec::new();
    for (line, info, body) in fenced_blocks() {
        match info.as_str() {
            "" => examples.push(Example {
                origin: format!("SPECIFICATION.md:{line}"),
                mode: mode_of_bare(&body),
                source: body,
            }),
            "yaml" => match serde_yaml::from_str::<Yaml>(&body) {
                Ok(doc) => walk_yaml(
                    &doc,
                    Mode::Html,
                    &format!("SPECIFICATION.md:{line}"),
                    &mut examples,
                ),
                Err(e) => failures.push(format!(
                    "SPECIFICATION.md:{line}: yaml example does not parse: {e}"
                )),
            },
            _ => {}
        }
    }
    (examples, failures)
}

fn lower(source: &str) -> Result<Expr, String> {
    let parsed = kani_yaml::dsl::parse(source).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    Expr::try_from(parsed).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })
}

async fn evaluate(expr: Expr, mode: Mode) -> Result<(), String> {
    let container = match mode {
        Mode::Html => "body",
        Mode::Json => "",
    };
    let blueprint = BlueprintBuilder::new(container)
        .scalar_opt("base_url", Expr::Literal("https://example.com/".into()))
        .field_opt("value", expr)
        .build();
    let mut state = HostState::default();
    state.preferences.insert("language".into(), "en".into());
    state
        .preferences
        .insert("cover_size".into(), ".512.jpg".into());
    match mode {
        Mode::Html => html_eval::extract_html_str(&mut state, HTML_FIXTURE, &blueprint).await,
        Mode::Json => {
            json_eval::extract_json_str(&mut state, &json_fixture().to_string(), &blueprint).await
        }
    }
    .map(|_| ())
}

#[tokio::test]
async fn every_spec_example_parses_and_evaluates() {
    let (examples, mut failures) = collect_examples();

    assert!(
        examples.len() >= 64,
        "found only {} DSL examples; the extractor has probably stopped matching the spec",
        examples.len()
    );

    for example in &examples {
        let expr = match lower(&example.source) {
            Ok(expr) => expr,
            Err(e) => {
                failures.push(format!("{}: does not parse: {e}", example.origin));
                continue;
            }
        };
        if example.source.contains(".user.") {
            continue;
        }
        if let Err(e) = evaluate(expr, example.mode).await {
            failures.push(format!(
                "{}: fails to evaluate ({:?}): {e}",
                example.origin, example.mode
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} spec example(s) are broken:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

struct HookBlock {
    origin: String,
    hooks: kani_core::scripting::HookScripts,
    pure: std::collections::BTreeMap<String, String>,
    endpoints: Vec<String>,
}

fn string_map(value: Option<&Yaml>) -> std::collections::BTreeMap<String, String> {
    value
        .and_then(Yaml::as_mapping)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| Some((k.as_str()?.to_owned(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn hook_blocks() -> Vec<HookBlock> {
    let mut blocks = Vec::new();
    for (line, info, body) in fenced_blocks() {
        if info != "yaml" {
            continue;
        }
        let Ok(doc) = serde_yaml::from_str::<Yaml>(&body) else {
            continue;
        };
        let mut hooks = kani_core::scripting::HookScripts {
            pre_request: doc
                .get("pre_request")
                .and_then(Yaml::as_str)
                .map(str::to_owned),
            on_status: string_map(doc.get("on_status")),
            ..Default::default()
        };
        let mut endpoints = Vec::new();
        if let Some(map) = doc.get("endpoints").and_then(Yaml::as_mapping) {
            for (name, endpoint) in map {
                let Some(name) = name.as_str() else { continue };
                endpoints.push(name.to_owned());
                if let Some(body) = endpoint.get("pre_request").and_then(Yaml::as_str) {
                    hooks
                        .endpoint_pre_request
                        .insert(name.to_owned(), body.to_owned());
                }
                let on_status = string_map(endpoint.get("on_status"));
                if !on_status.is_empty() {
                    hooks.endpoint_on_status.insert(name.to_owned(), on_status);
                }
            }
        }
        let pure = string_map(doc.get("scripts").and_then(|s| s.get("pure")));
        hooks.cache = doc
            .get("cache")
            .and_then(Yaml::as_mapping)
            .map(|map| {
                map.iter()
                    .filter_map(|(name, entry)| {
                        let limits = kani_shared::CacheNamespaceLimits {
                            ttl_seconds: entry
                                .get("ttl")
                                .and_then(Yaml::as_u64)
                                .map_or(3600, |t| t as u32),
                            max_entries: entry
                                .get("max_entries")
                                .and_then(Yaml::as_u64)
                                .map(|m| m as u32),
                        };
                        Some((name.as_str()?.to_owned(), limits))
                    })
                    .collect()
            })
            .unwrap_or_default();
        hooks.shared = pure.clone();
        if !hooks.is_empty() || !pure.is_empty() {
            blocks.push(HookBlock {
                origin: format!("SPECIFICATION.md:{line}"),
                hooks,
                pure,
                endpoints,
            });
        }
    }
    blocks
}

fn hook_ctx() -> kani_core::scripting::bindings::ScriptableCtx {
    kani_core::scripting::bindings::ScriptableCtx {
        cache_backend: std::sync::Arc::new(kani_core::cache::InMemoryCache::new()),
        cache_namespace: "spec:".into(),
        prefs: std::collections::HashMap::new(),
        v8_process: None,
        http: None,
        browser_scripts: None,
        browser_profile_key: None,
        allowed_host: kani_core::wasm::AllowedHost::MetadataOnly,
        cache_namespaces: std::sync::Arc::default(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_spec_hook_example_compiles_and_runs() {
    use kani_core::evaluator::shared::Value;
    use kani_core::scripting::bindings::ScriptableRequest;

    let blocks = hook_blocks();
    let hook_count: usize = blocks
        .iter()
        .map(|b| {
            usize::from(b.hooks.pre_request.is_some())
                + b.hooks.on_status.len()
                + b.hooks.endpoint_pre_request.len()
                + b.hooks
                    .endpoint_on_status
                    .values()
                    .map(|m| m.len())
                    .sum::<usize>()
                + b.pure.len()
        })
        .sum();
    assert!(
        hook_count >= 5,
        "found only {hook_count} hook bodies; the extractor has probably stopped matching the spec"
    );

    let mut failures = Vec::new();
    for block in &blocks {
        match kani_core::scripting::HookRegistry::compile(&block.hooks) {
            Err(e) => failures.push(format!("{}: hooks do not compile: {e}", block.origin)),
            Ok(registry) => {
                for (hook, name) in registry.unresolved_calls() {
                    failures.push(format!(
                        "{}: {hook} calls `{name}`, which Kani does not define",
                        block.origin
                    ));
                }
                for (hook, namespace) in registry.undeclared_cache_namespaces() {
                    failures.push(format!(
                        "{}: {hook} uses cache namespace '{namespace}' without declaring it",
                        block.origin
                    ));
                }
                let ids = block
                    .endpoints
                    .iter()
                    .map(|e| Some(e.as_str()))
                    .chain([None]);
                for endpoint_id in ids {
                    let mut req = ScriptableRequest {
                        method: "GET".into(),
                        url: "https://example.com/".into(),
                        headers: Vec::new(),
                        queries: Vec::new(),
                        body: None,
                        endpoint_id: endpoint_id.map(str::to_owned),
                    };
                    if let Err(e) = registry.run_pre_request(&mut req, hook_ctx()) {
                        failures.push(format!(
                            "{}: pre_request for {endpoint_id:?} fails: {e}",
                            block.origin
                        ));
                    }
                }
            }
        }
        match kani_core::scripting::PureFunctionRegistry::compile(&block.pure) {
            Err(e) => failures.push(format!(
                "{}: pure scripts do not compile: {e}",
                block.origin
            )),
            Ok(registry) => {
                for name in block.pure.keys() {
                    match registry.call(name, &[Value::Str("Hello World".into())]) {
                        Ok(Value::Str(_)) => {}
                        other => failures.push(format!(
                            "{}: pure function {name} does not return a string: {other:?}",
                            block.origin
                        )),
                    }
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} spec hook example(s) are broken:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[tokio::test]
async fn literals_are_numbers_and_bools_are_output() {
    let blueprint = BlueprintBuilder::new("")
        .field("sum", lower("2 + 2").unwrap())
        .field("parsed", lower("\"7\".parse_int()").unwrap())
        .field("flag", lower("true").unwrap())
        .scalar("has_next_page", lower("1 < 2").unwrap())
        .build();
    let mut state = HostState::default();
    let out = json_eval::extract_json_str(&mut state, "{}", &blueprint)
        .await
        .unwrap();
    let row = &out["rows"][0];

    assert_eq!(
        lower("2").unwrap(),
        Expr::Number(2.0),
        "a literal is a Number"
    );
    assert!(row["sum"].is_f64(), "2 + 2 is a Number: {}", row["sum"]);
    assert_eq!(row["sum"].as_f64(), Some(4.0));
    assert!(
        row["parsed"].is_i64(),
        "parse_int gives an Int: {}",
        row["parsed"]
    );
    assert_eq!(row["flag"], serde_json::json!(true));
    assert_eq!(out["scalars"]["has_next_page"], serde_json::json!(true));
}

#[test]
fn an_endpoint_hook_replaces_the_source_hooks_setup() {
    use kani_core::scripting::bindings::ScriptableRequest;
    let block = hook_blocks()
        .into_iter()
        .find(|b| b.pure.contains_key("bearer"))
        .expect("the §3.10 replacement example");
    let registry = kani_core::scripting::HookRegistry::compile(&block.hooks).unwrap();
    let auth_for = |endpoint: Option<&str>| {
        let mut req = ScriptableRequest {
            method: "GET".into(),
            url: "https://example.com/".into(),
            headers: Vec::new(),
            queries: Vec::new(),
            body: None,
            endpoint_id: endpoint.map(str::to_owned),
        };
        let mut ctx = hook_ctx();
        ctx.prefs.insert("api_token".into(), "t0k".into());
        registry.run_pre_request(&mut req, ctx).unwrap();
        req.headers
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.clone())
    };

    assert_eq!(
        auth_for(Some("popular")).as_deref(),
        Some("Bearer t0k"),
        "source hook"
    );
    assert_eq!(
        auth_for(Some("search")).as_deref(),
        Some("Bearer t0k"),
        "endpoint hook sets it"
    );
    assert_eq!(
        auth_for(Some("chapter_list")),
        None,
        "endpoint hook replaced the source hook"
    );
}
