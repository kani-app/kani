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
