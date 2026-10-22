# Contributing to Kani

## Branching

Permanent branches: `main` (stable releases) and `develop` (integration).

- Branch new work off `develop`: `git checkout -b feature/<short-name> develop`
- Keep each branch to one feature or a tight set of related changes. Split if concerns diverge.
- Open a PR into `develop` before starting the next feature.
- `main` is updated from `develop` at release points only. Never commit directly to `main` or `develop`.

## First-time setup

```bash
cargo run -p kani-cli -- setup   # downloads JS vendors, Tailwind CLI, esbuild; installs git hooks
```

Requires: Rust stable (targets `wasm32-unknown-unknown`), `wasm-tools`, `wasm-opt`, `sqlx-cli`.

## Building and testing

```bash
cargo build --release

# Tests (omit --workspace — extension crates can't compile native):
cargo test
cargo test -p kani-core <test_name>
cargo test -p kani-app --lib

# Lint (unwrap_used is denied workspace-wide):
cargo clippy -- -D warnings

# Build a WASM extension:
cargo run -p kani-cli -- build kani-example
cargo run -p kani-cli -- build kani-<source> --ext-dir ../kani-extensions
cargo run -p kani-cli -- build --all
```

See the developer documentation for [local development](docs/docs/developer/local-dev.md) and
[test conventions](docs/docs/developer/test-conventions.md).

## SQL schema changes

After any migration, regenerate the SQLx query cache:

```bash
cargo sqlx prepare --workspace -- --all-targets
```

Commit the updated `.sqlx/` directory. CI checks for staleness with `cargo sqlx prepare --check`.

## PR expectations

- Write tests with new logic. Every new pure function gets a happy-path test and an
  edge/error test. New REST endpoints get a test triplet: 200 authed, 401 unauthed, 4xx invalid.
- Keep commits atomic and branches focused. After review begins, prefer a new commit to an amendment.
- Reference the issue number in the PR description if applicable.
- CI must be green (build, test, clippy, sqlx check) before merge.

## Code style

- **No comments unless clearly required** (safety notes, doc-tests). Write clear names instead.
- Frontend: use `t("key")` from `static/js/i18n.js` for all user-visible strings. Add the key and
  English value to `static/locales/en.js`.
- Design tokens live in `static/css/app.css`. Never hard-code colours, radii, or shadows in JS/CSS.
