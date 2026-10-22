# Architecture

Kani is an Axum application and vanilla-JS/Preact SPA built around sandboxed content-source
extensions. The service layer owns durable behavior; the web crate translates HTTP to service
calls; `kani-core` hosts WASM, networking, extraction, downloads, archives, and scripting.

## Layers

```text
browser / OPDS / API client
          |
       kani-web          HTTP, auth, permissions, CSRF, metrics, static frontend
          |
       kani-app          services, SQLite, jobs, sources, integrations
          |
       kani-core         WASM host, HTTP, extraction, download/archive machinery
          |
      kani-shared        guest/host ABI types and extraction AST
```

`kani-yaml` parses and validates declarative extensions. `kani-cli` uses it for authoring and build
workflows. `kani-lease` isolates source hot-swap lease/drain coordination. Test-support crates and
WASM-only extension members sit beside the runtime crates.

## WASM component interface

`kani-core/wit/kani.wit` defines the `kani-extension` world. The host imports HTTP, HTML, JSON,
utility, preferences, extraction, cache, and scripting interfaces into the guest. The guest exports
`manga-provider`.

Host calls are asynchronous. Guest bindings appear synchronous for most imports, but
Wasmtime suspends the component fiber while the host awaits I/O. The chapter-list stream export is
a component-model async stream; the default guest bridge produces it from page-granular chapter
calls.

HTML documents, element lists, and JSON trees cross the boundary as opaque integer handles. The
host owns their storage. Guest RAII wrappers release handles explicitly and must not be bypassed by
retaining stale raw integers.

## Declarative extraction

An extension sends a serialized `Blueprint` rather than making hundreds of selector calls over
FFI. A blueprint contains a container, fields, bindings, document scalars, optional request, and
chained-fetch behavior. The HTML or JSON evaluator returns rows and scalars in one JSON tree.

The shared `Expr` AST covers DOM and JSON navigation, lists, strings, dates, URLs, control flow,
preferences, scalar references, and user functions. `postcard` is used for the compact guest/host
blueprint encoding.

Declarative YAML uses the same AST and evaluators as Rust-authored blueprints. This is an important
contract: a feature wired only into code generation or only into interpreted YAML is not complete.

## Source lifecycle

Installed source metadata and artifact paths are stored in SQLite. `SourceRegistry` holds active
backends. Installation verifies compatibility and, for repository sources, trust, digest, and
signatures before persisting or inserting the backend.

Updates are serialized per extension ID. Hot-swap uses leases so an in-flight call can finish on
the old backend while new calls move to the replacement. A failed verification or load must leave
the previous source usable.

## Application service

`AppService` is the shared application handle. It owns write and read pools, database path,
`WasmRuntime`, source registry, settings cache, download manager, smart HTTP clients, event
broadcast, request and extension caches, cancellation, trackers, email, encryption, degradations,
webhooks, metadata providers, background jobs, update state, and per-source install locks.

Services contain business logic and SQL without depending on Axum. The web crate's `AppState`
wraps the service and adds HTTP concerns such as authentication backend, rate limiting, CSRF,
metrics, and static delivery.

## Background work

Long-lived work implements `BackgroundJob` and runs through `JobManager`. Recurring work is a
`RecurringJobKind` dispatched by the recurring scheduler. This provides persistence, progress,
retry, cancellation, deduplication, and UI visibility instead of unrelated `tokio::spawn` loops.

Request-scoped side effects may still spawn a task when they need none of those properties.

## HTTP and authorization

REST routers are composed under `/rest`. Global middleware handles tracing, request limits,
security headers, rate limits, sessions, API-token authentication, CSRF, and permission guards.
Some top-level endpoints bypass cookie middleware and perform their own authentication.

Each protected handler declares an `AuthRequirement`. Permissions are parsed `resource:action`
values shared with roles and frontend gating. OpenAPI is generated from handler annotations and is
served only in debug builds.

## Frontend

Source JavaScript lives under `static/js`; esbuild outputs `static/js/dist`. Pages are loaded by the
SPA router. New UI is Preact/htm, while documented large or performance-sensitive pages retain
vanilla hosts and Preact islands.

State is separated into session identity (`session.js`), SSE-fed server cache (`cache.js`), and
browser-local UI state (`ui-state.js`). `api.js` owns HTTP calls, `sse.js` owns live events, and the
flat English catalogue in `static/locales/en.js` owns visible copy.

## Persistence

SQLite migrations live under `migrations/`; SQLx offline metadata is committed under `.sqlx`. See
[Migrations](migrations.md) for the procedures governing changes to either.
Downloaded files and manifests live under configured storage volumes. Generated encryption keys
are sidecars to the database and are part of disaster recovery.
