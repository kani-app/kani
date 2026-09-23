# Kani

[![CI](https://github.com/kani-app/kani/actions/workflows/ci.yml/badge.svg)](https://github.com/kani-app/kani/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/kani-app/kani/graph/badge.svg?token=H9PGTX76j2)](https://codecov.io/gh/kani-app/kani)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Kani is a self-hosted manga library server written in Rust with a WebAssembly extension system.
Inspired by [Tachiyomi/Mihon](https://mihon.app/) and [Komga](https://komga.org/).

> **You are looking at `develop`,** the default branch and the integration line for approved work.
> It is ahead of the latest release and may contain unreleased changes. `main` tracks releases
> only. To run a released version, use a tagged image or check out a release tag.

<!--screenshot: docs/screenshots/library.png — library grid view with category tabs and filters-->
<!--screenshot: docs/screenshots/reader.png — chapter reader in scroll mode-->

---

## Features

### Library

- Manga library with custom metadata, categories, and favourites
- Cover, name, and description overrides; per-manga notes
- Reading progress tracked per chapter and page
- Continue-reading shelf
- Auto-scan for new chapters (global and per-manga)
- Scanlator and language preferences (priority + blocking)
- Duplicate detection and merge

### Downloads

- Configurable download rules (language, scanlator, volume filters, chained logic)
- Bulk download with per-chapter cancellation and history
- CBZ archive output
- Tachiyomi backup import with source mapping and duplicate handling
- Full library backup and restore

### Sources & Extensions

- Official source extensions, installed from a signed extension repository — the catalogue is
  maintained separately and is not bundled with the server
- WebAssembly Component extension system — sandboxed and fast
- Declarative YAML + DSL authoring for new sources — no raw Rust required
- CLI tooling: scaffold, validate, generate, and build extensions

### Integrations

- Tracker sync: AniList and MyAnimeList (OAuth 2.0, read/write)
- Outbound webhooks with per-manga overrides and a delivery log
- SMTP email (password reset, email verification, test send)
- OPDS 1.2 feed with page streaming (PSE) for e-reader clients

### Multi-user

- Role-based access control with admin and user roles
- Granular per-resource permissions
- User and role management UI
- Audit log
- Optional credential encryption at rest (ChaCha20-Poly1305)

### Export

- Chapter export to EPUB and KEPUB
- Kindle/MOBI export via KCC (requires optional Docker build arg — see below)

---

## Quick Start

### Docker (recommended)

```bash
git clone https://github.com/kani-app/kani.git
cd kani
docker compose up --build
```

Open <http://localhost:8242> and create the administrator account. Setup closes after the account
is created.

Setup accepts requests from loopback and private addresses only. To set up a directly exposed
server, set `KANI_ALLOW_REMOTE_SETUP=true`; use this option only when a tunnel or reverse proxy is
unavailable.

Data is persisted in two directories created alongside `docker-compose.yml`:

| Directory | Contents |
|-----------|----------|
| `./data`  | SQLite database, installed WASM extensions |
| `./library` | Manga covers, downloaded chapters |

**Optional features** — uncomment the relevant args in `docker-compose.yml` before building:

```yaml
build:
  args:
    INSTALL_KCC: "true"       # Kindle Comic Converter — required for MOBI/AZW3 export
```

### Local build

Prerequisites: Rust stable, `wasm32-unknown-unknown` target,
[`wasm-tools`](https://github.com/bytecodealliance/wasm-tools),
[`wasm-opt`](https://github.com/WebAssembly/binaryen).

```bash
cargo run -p kani-cli -- setup        # fetch JS vendors, Tailwind CLI, and configure git hooks
cargo build --release                 # build the server binary
cargo run -p kani-cli -- build --all  # compile WASM extensions
./target/release/kani-web
```

---

## Configuration

Set these in `docker-compose.yml` or pass as `-e` flags:

| Variable | Default | Description |
|----------|---------|-------------|
| `KANI_BIND` | `0.0.0.0:8242` | Listen address |
| `KANI_SECURE_COOKIES` | `false` | Set `true` when behind a TLS-terminating proxy |
| `KANI_TRUSTED_PROXIES` | *(none)* | Comma-separated IPs/CIDRs whose `X-Forwarded-For` is believed. Required when behind a reverse proxy, or every client shares one rate-limit bucket |
| `KANI_CORS_ORIGIN` | *(mirrors request)* | Restrict CORS to a specific origin in production |
| `KANI_SECRET_KEY` | *(none)* | 32-byte hex key for credential encryption at rest (`openssl rand -hex 32`) |
| `KANI_SECRET_KEY_FILE` | *(none)* | Load the encryption key from a file (for Docker secrets) |
| `KANI_SOLVER_SECRET` | *(none)* | Shared key sent to the solver; must match its `API_KEY` |
| `RUST_LOG` | `error` | Log level: `error`, `warn`, `info`, `debug`, `trace` |

---

## Extension System

Extensions are WebAssembly Components compiled against a [WIT](kani-core/wit/kani.wit) interface.
The host provides HTTP, HTML/JSON extraction, utilities, and preferences; the extension exports a
`manga-provider` implementation. Extensions are sandboxed — they can only reach the host APIs.

Extensions can be defined in YAML with a declarative schema and extraction DSL:

```bash
cargo run -p kani-cli -- new my-source           # scaffold a YAML template
cargo run -p kani-cli -- validate my-source.yaml # check the schema
cargo run -p kani-cli -- generate my-source.yaml # generate a Rust crate
cargo run -p kani-cli -- build kani-my-source    # compile to WASM
```

See [`SPECIFICATION.md`](SPECIFICATION.md) for the full DSL reference and YAML schema.

---

## Development

```bash
cargo test                      # run all tests
cargo test -p kani-app --lib    # lib tests only (no DB required)
cargo clippy -- -D warnings
cargo fmt --check
```

`unwrap_used` is denied workspace-wide. Use `?`, `expect`, or an explicit match.

### Architecture

The workspace has six crates:

| Crate | Role |
|-------|------|
| `kani-core` | WASM runtime, host ABI, extraction engine, CBZ/ComicInfo, downloader, V8 subprocess |
| `kani-shared` | Shared types and traits used by both host and guest |
| `kani-app` | Business logic: library, downloads, trackers, webhooks, backup/restore, export, encryption, audit |
| `kani-web` | Axum HTTP server, REST API, RBAC auth, frontend serving |
| `kani-cli` | Extension tooling: YAML schema, DSL parser, codegen, build orchestration, CSS/setup |
| `kani-extensions/*` | Individual extension WASM modules |

Extensions are sandboxed WASM Components built against a WIT interface (`kani-core/wit/kani.wit`).
Extensions submit a declarative `Blueprint` in one FFI call. The host evaluates it against parsed
HTML or JSON.
See `SPECIFICATION.md` for the full extension authoring reference.

### Testing

| Kind | Location | When to use |
|------|----------|-------------|
| Unit (pure functions, no I/O) | `#[cfg(test)] mod tests` in the source file | Default for any new pure function |
| Integration (DB-backed service logic) | `kani-app/tests/<area>_tests.rs` | New service methods that touch SQLite |
| REST API | `kani-web/tests/<area>_api_tests.rs` | New HTTP endpoints |
| CLI / codegen | `kani-cli/tests/` | YAML validation rules, DSL changes, codegen output |

Every new pure function requires a happy-path test and at least one edge or error case. New REST
endpoints require tests for an authenticated `200`, unauthenticated `401`, and invalid-input `4xx`.
Use `common::test_state()` + `common::build_test_app()` + `common::create_admin()` from
`kani-web/tests/common/mod.rs`; this wires the full auth stack against an in-memory SQLite DB.

After any SQL schema change run `cargo sqlx prepare --workspace -- --all-targets --tests` to
regenerate the `.sqlx/` query cache. The pre-push hook (configured by `setup`) will catch a stale
cache before it reaches CI, provided `sqlx-cli` is installed (`cargo binstall sqlx-cli`).

### Contributing

Branch from `develop` (`git checkout -b feature/<name> develop`) and open a PR into `develop`. Keep
each branch focused on one feature or a closely related set of changes. Update `main` from
`develop` only for releases. `develop` is the default branch, so a new pull request targets it
without needing an explicit base.

---

## Legal

Kani does not distribute content or third-party extensions. See the [Disclaimer](DISCLAIMER.md).

---

## License

MIT — see [LICENSE](LICENSE)
