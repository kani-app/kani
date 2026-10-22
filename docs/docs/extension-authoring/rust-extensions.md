# Rust Extensions

Use a Rust extension when the declarative YAML model cannot express the source or when you need
direct control over the guest implementation. Prefer YAML for ordinary request-and-extraction
sources because it has less boilerplate and can use the interpreted backend.

## Scaffold and build

```bash
cargo run -p kani-cli -- new my-source --rust
cargo run -p kani-cli -- build kani-my-source
```

Extension crate names conventionally start with `kani-`. Production builds are written to
`wasm_sources/<name>.wasm`. Use `--debug` for readable WASM backtraces. Build extensions through
`kani-cli`; a direct native `cargo build -p` targets the wrong platform and component pipeline.

## Provider implementation

Implement `kani_shared::MangaExtension` for the source logic. The WIT-generated `Guest` trait is a
thin adapter that delegates to one extension instance, conventionally stored in a `OnceLock`.

Provider operations cover metadata, popular/search listings, manga details, chapters, pages,
filters, preferences, URL generation, and optional sort behavior. Return
`ExtensionResult<T>`, using structured `ExtensionError` kinds for network, parse, not-found,
rate-limit, timeout, and invalid-input failures.

## WIT boundary

The `kani-extension` world imports host interfaces for HTTP, HTML, JSON, utility, preferences,
declarative extraction, cache, and scripting. It exports `manga-provider`.

HTML documents, element lists, and JSON trees cross the boundary as opaque integer handles.
`HtmlDocument` and `JsonHandle` wrappers in `kani-shared::host_abi` release owned handles on drop.
Do not retain a raw handle after its owner is freed.

Host imports are asynchronous in Wasmtime even though guest Rust calls their generated interface
synchronously. Wasmtime suspends the component fiber while the host awaits I/O.

## Extraction blueprints

Prefer one declarative `Blueprint` over repeated DOM calls:

```rust
use kani_shared::ast::{BlueprintBuilder, Expr};

let blueprint = BlueprintBuilder::new(".item")
    .field("title", Expr::self_ref().first("h3").text().trim())
    .field("url", Expr::self_ref().first("a").attr("href"))
    .build();
```

Attach an HTTP request to the blueprint when the helper supports it, then evaluate with
`host_abi::extract::html` or `extract::json`. Results use rows and document scalars and are decoded
through `JsonHandle` accessors.

## Chapter streaming

The WIT export `get-chapter-list-stream` is a component-model async stream. Current extensions can
implement it with `bridge_chapter_list_stream`, which repeatedly calls the page-granular
`get_chapter_list`. Production chapter loading currently uses ordinary paged calls because shipped
sources are page-granular; do not promise sub-page delivery from the default bridge.

## Test and debug

- Put pure guest logic behind ordinary Rust functions and unit-test it where it compiles natively.
- Use `kani-shared-test` fixtures and local origins for deterministic request/extraction tests.
- Build dev fixtures with `cargo run -p kani-cli -- build --dev`.
- Run the host ABI contract tests in `kani-core/tests/wasm_abi.rs` after changing bindings or the
  WIT interface.
- Record a minimal failing response rather than depending only on a live upstream page.

Changing WIT, `MangaExtension`, or shared cross-boundary types affects every host and extension and
requires an explicit compatibility analysis.
