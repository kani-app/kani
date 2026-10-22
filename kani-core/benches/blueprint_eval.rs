use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kani_core::evaluator::html_eval::extract_html_str;
use kani_core::evaluator::json_eval::extract_json_str;
use kani_core::wasm::HostState;
use kani_shared::ast::{Blueprint, Expr, FieldDef};

const MANGA_HTML: &str = include_str!("fixtures/manga_list.html");
const MANGA_JSON: &str = include_str!("fixtures/manga_list.json");

fn field(name: &str, expr: Expr) -> FieldDef {
    FieldDef {
        name: name.into(),
        expr,
        optional: false,
    }
}

fn first(selector: &str) -> Expr {
    Expr::First {
        target: Box::new(Expr::SelfRef),
        selector: selector.into(),
    }
}

fn text_of(selector: &str) -> Expr {
    Expr::Text {
        target: Box::new(first(selector)),
    }
}

fn attr_self(name: &str) -> Expr {
    Expr::Attr {
        target: Box::new(Expr::SelfRef),
        name: name.into(),
    }
}

fn attr_of(selector: &str, name: &str) -> Expr {
    Expr::Attr {
        target: Box::new(first(selector)),
        name: name.into(),
    }
}

fn blueprint(container: &str, fields: Vec<FieldDef>) -> Blueprint {
    Blueprint {
        request: None,
        container: container.into(),
        fields,
        bindings: vec![],
        scalars: vec![],
        pagination: None,
    }
}

fn html_blueprint() -> Blueprint {
    blueprint(
        ".container article.manga-card",
        vec![
            field("id", attr_self("data-id")),
            field("title", text_of("h2.title")),
            field("cover", attr_of("img.cover", "data-src")),
            field("status", text_of("span.status")),
            field("description", text_of("p.desc")),
            field("chapters", text_of("span.chapter-count")),
        ],
    )
}

fn json_blueprint() -> Blueprint {
    let path = |p: &str| Expr::JsonStr {
        target: Box::new(Expr::JsonPtr {
            target: Box::new(Expr::SelfRef),
            pointer: format!("/{p}"),
        }),
    };
    blueprint(
        "/data/items",
        vec![
            field("id", path("id")),
            field("title", path("title")),
            field("cover", path("cover")),
            field("status", path("status")),
            field(
                "chapters",
                Expr::JsonInt {
                    target: Box::new(Expr::JsonPtr {
                        target: Box::new(Expr::SelfRef),
                        pointer: "/chapters".into(),
                    }),
                },
            ),
        ],
    )
}

fn bench_evaluators(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("tokio runtime");

    let html_bp = html_blueprint();
    let json_bp = json_blueprint();

    let mut group = c.benchmark_group("blueprint_eval");

    group.bench_function("html_200_rows", |b| {
        b.iter(|| {
            let mut state = HostState::default();
            let out = rt
                .block_on(extract_html_str(
                    &mut state,
                    black_box(MANGA_HTML),
                    black_box(&html_bp),
                ))
                .expect("html extraction");
            black_box(out);
        })
    });

    group.bench_function("json_200_rows", |b| {
        b.iter(|| {
            let mut state = HostState::default();
            let out = rt
                .block_on(extract_json_str(
                    &mut state,
                    black_box(MANGA_JSON),
                    black_box(&json_bp),
                ))
                .expect("json extraction");
            black_box(out);
        })
    });

    group.finish();
}

criterion_group!(benches, bench_evaluators);
criterion_main!(benches);
