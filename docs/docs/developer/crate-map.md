# Crate Map

## Workspace layout

| Member | Responsibility |
|---|---|
| `kani-web` | Axum server, REST/OPDS routes, auth, metrics, and frontend delivery |
| `kani-app` | SQLite-backed services, source registry, integrations, and jobs |
| `kani-core` | WASM host, HTTP/extraction, scripting, downloads, CBZ, and ComicInfo |
| `kani-shared` | WIT guest bindings, provider traits/types, handles, and extraction AST |
| `kani-yaml` | Declarative schema, DSL parser, validation, and interpreted model |
| `kani-cli` | Authoring, build, signing, CSS, setup, and diagnostic commands |
| `kani-lease` | Loom-tested lease and drain primitive used for safe hot-swap |
| `kani-shared-test` | Deterministic local-origin and cross-crate test support |
| `kani-extensions/*` | WASM-only source components and ABI fixtures |

Native `default-members` include the seven production host/tool crates and exclude extension
members plus test support where appropriate. Use `cargo test` and `cargo clippy` without
`--workspace`; a workspace-wide native build tries to link WASM-only guests.

## Dependency direction

```text
kani-web -> kani-app -> kani-core -> kani-shared
                |             |
                |             +-> kani-lease
                +-> kani-yaml -> kani-shared

kani-cli -> kani-yaml -> kani-shared
         -> kani-core

kani-extensions/* -> kani-shared
kani-shared-test  -> test consumers
```

Consult `Cargo.toml` for the exact feature-gated edges. `kani-shared` is intentionally usable by
both host and guest, but it is not a zero-dependency crate; host-only SQLx and serialization
features are optional.

## Important module groups

### `kani-core`

- `wasm/` and runtime modules instantiate components and implement host imports.
- `evaluator/` evaluates blueprints against HTML and JSON.
- `http/` applies request policy, rate limits, retries, and response budgets.
- `scripting/` owns the Rhai sandbox.
- downloader, CBZ, ComicInfo, quality, and V8 modules own content processing.

### `kani-app`

- `service/` is organized by library, chapters, downloads, imports, sources, repositories,
  trackers, webhooks, email, backup, storage, diagnostics, security, and other domains.
- `jobs/` contains the background-job framework and concrete recurring or submitted work.
- `source/` owns backend registration and hot-swap.
- `permissions.rs`, typed IDs, models, cache, events, and settings support the services.

### `kani-web`

- `rest/` contains domain routers and handler schemas.
- `auth.rs`, `csrf.rs`, and permission guards implement browser and token authorization.
- `app.rs` builds the testable router; `main.rs` adds process startup and production serving.
- `openapi.rs` registers REST handler schemas and is contract-tested against routing.
- metrics, OPDS, proxy, static, security-header, and rate-limit modules own top-level surfaces.

### `kani-yaml` and `kani-cli`

`kani-yaml` is the reusable declarative implementation. The CLI should orchestrate it, not carry a
second schema. CLI commands cover scaffold, validate, generate, build, DSL and REPL tools, key and
repository management, frontend assets, quality checks, and archive diagnostics.
