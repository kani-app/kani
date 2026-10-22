# Local Development

## Prerequisites

- Stable Rust with the `wasm32-unknown-unknown` target.
- Node.js for frontend checks and bundling.
- `wasm-tools` and Binaryen's `wasm-opt` for extension components.
- Python with MkDocs Material for documentation.
- Docker when exercising the production image or external dependencies.

## First setup

```bash
cargo run -p kani-cli -- setup
```

Setup downloads Preact/htm vendor files, Tailwind's standalone CLI, esbuild, and configures the git
hooks. Re-run the relevant setup mode when a downloaded tool or vendor asset is missing.

## Run the server and frontend

```bash
cargo run -p kani-web
```

Open `http://localhost:8242`. Debug builds also expose Swagger UI at `/api-docs`.

For live CSS rebuilds:

```bash
cargo run -p kani-cli -- css --watch
```

JavaScript source lives in `static/js`; never edit `static/js/dist`. Production builds run the
bundling pipeline through the web crate's build script.

## Build extensions

```bash
cargo run -p kani-cli -- build kani-example
cargo run -p kani-cli -- build kani-<source> --ext-dir ../kani-extensions
cargo run -p kani-cli -- build --all
cargo run -p kani-cli -- build --dev
```

The output directory is `wasm_sources`. `--all` excludes development and ABI fixtures. Do not use a
native `cargo build -p` for an extension.

## Test and lint

```bash
cargo test
cargo test -p kani-app --lib
cargo clippy --locked --no-deps -- -D warnings
cargo fmt --all --check
```

Do not add `--workspace`: WASM-only extension crates cannot link for the native target. Run focused
DB-backed or REST tests while developing, then the default-member suite before handoff.

`unwrap_used` is denied. Use error propagation or an explicit branch; `expect` is reserved for a
condition whose invariant is clear from context.

## SQLx metadata

After a schema or checked-query change:

```bash
cargo sqlx prepare --workspace -- --all-targets
```

Commit `.sqlx` changes. Build without a live database with `SQLX_OFFLINE=true`.

## Dependency changes

After adding, removing, or changing the features of a dependency, or adding a native workspace
crate:

```bash
cargo install cargo-hakari --locked
cargo hakari generate
cargo hakari manage-deps
```

Commit the `workspace-hack` changes. The crate pins one feature set for every native crate, so the
release builds each package with the dependencies CI tests; CI fails when it is stale.

## Frontend checks

```bash
node scripts/check-i18n-keys.js
node scripts/check-untranslated-strings.js
node scripts/check-sanitize-css-parity.mjs
node scripts/audit-tokens.mjs --check --max 0
```

Visible strings use `t("key")`; English values live in `static/locales/en.js`. Color, radius,
shadow, motion, and z-index values use design tokens.

## Documentation

```bash
cd docs
mkdocs serve
mkdocs build --strict
```

Add every authored page to `mkdocs.yml`. `docs/site` is generated and ignored. Planning notes do
not belong in the published docs tree.
