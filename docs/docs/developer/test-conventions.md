# Test Conventions

## Test placement

| Kind | Location | Use |
|---|---|---|
| Pure unit | `#[cfg(test)] mod tests` beside the code | New deterministic logic |
| Service integration | `kani-app/tests/<area>_tests.rs` | SQLite-backed service behavior |
| REST contract | `kani-web/tests/<area>_api_tests.rs` | Routing, auth, validation, and serialization |
| CLI/YAML | `kani-cli/tests` and `kani-yaml` tests | Schema, DSL, generation, and commands |
| ABI/runtime | `kani-core/tests` | WIT, WASM host, extraction, and network behavior |
| Browser contract | repository scripts | Permission-gated and responsive UI workflows |

Standalone test files begin with `#![allow(clippy::unwrap_used)]`. Commit accepted Insta snapshots
and never commit `.snap.new` files.

## Minimum behavior coverage

- Give every new pure function a happy path and an edge or error case.
- Test DB-backed service methods through the shared test service.
- Give a new REST endpoint authenticated success, unauthenticated failure, and invalid-input
  coverage, plus permission denial where authentication alone is insufficient.
- A rejection test must assert *why* it was rejected whenever the fixture could trip an earlier
  check. `assert!(result.is_err())` says only that something objected. If the input is corrupted,
  truncated, or otherwise malformed, parsing or reading usually objects first, and the check the
  test is named after never runs — `repo_add_rejects_tampered_artifact` flipped a byte in a signed
  YAML and passed with signature verification removed entirely, because the corrupted file no
  longer parsed. Either tamper in a way only the check under test can catch, or assert on the
  error. Where the function under test has a single plausible failure mode, `is_err()` is enough.
- Prefer one harness over many near-identical tests. If a test differs from its neighbours only in
  a route, an id, or an expected status, it belongs in a table or a contract test driven from the
  app's own definitions rather than in a function of its own. Per-route copies cost a test each to
  assert one fact and still only cover the routes somebody remembered to write; deriving the list
  from the router covers every route and makes a new one covered the moment it is mounted.
  `auth_guard_contract_tests` and `permission_guard_contract_tests` replace 124 such tests between
  them. The trade is legibility of failure, so a harness must name the offending route in its
  assertion message.
- Seed through `common`, not a local copy. `insert_source` / `insert_manga` / `insert_chapter` and
  `seed_manga_with_chapter` already exist; four test files had grown their own near-identical
  version. Likewise `admin_app()` replaces the four-line `test_state` → `create_admin` →
  `build_test_app` → `login` preamble.
- Assert what the success case produced, not only its status code. A status assertion establishes
  that the route is mounted and the guard admitted the caller; it holds just as well when the
  handler returns an empty list, the wrong records, or another user's rows. Check a field of the
  response body or the row the request should have written. Refusal cases are the exception —
  for a 401 or 404 the status is the behaviour under test. `scripts/check-test-assertions.mjs`
  enforces this and carries a baseline of tests that predate it.
- Test what a shared-path change displaces: rename, delete, restore, retry, migration, re-download,
  and rollback paths are common hidden consumers.
- Use deterministic local HTTP origins or recorded fixtures instead of making unit tests depend on
  a live content site.

## REST helpers

`kani-web/tests/common/mod.rs` builds an `AppState`, router, administrator, login cookie, JSON
requests, and response decoding for integration tests. Read the current helper signatures before
using them; do not reproduce a second test harness in each file.

OpenAPI route coverage is a separate contract: adding or removing a REST route requires the
generated document and its coverage test to change together.

## Trace a signal to a pixel

For a new value, identify every hop:

```text
compute -> persist -> select -> serialize -> client cache -> render
```

Tests should fail when any required hop is disconnected. Pay special attention to hand-built JSON
objects, queries that omit new columns, settings saved but never read, and SSE handlers that update
state no component consumes.

## Permissions

The server contract test ensures every frontend permission literal parses. The permission-matrix
browser script then checks each gated surface with an account that has and lacks that permission.
Run it against a disposable instance because it creates roles and accounts:

```bash
node scripts/verify-permission-matrix.mjs http://127.0.0.1:8299 admin '<password>'
```

Install Playwright in a scratch environment and raise development rate limits for the sweep.

## Mobile layout

The design system's responsive rules are rendered-geometry properties — a touch-target floor,
tables that scroll inside a wrapper rather than widening the page, content that clears the bottom
nav. None can be checked from source. The layout script drives every route at several narrow
widths and asserts them:

```bash
node scripts/verify-mobile-layout.mjs http://127.0.0.1:8299 admin '<password>' \
  --min-gutter=16 --widths=320,390,412
```

Pass `--min-gutter=16`. It defaults to 0, which only catches content running
*under* the nav; 16px is the gutter the design system reserves above it, so the
default passes layouts the rule does not.

It reads the touch floor out of `design-system.md` and the route list out of `router.js`, so a
rule or a route that changes changes what it asserts. Useful flags: `--widths=320,390`,
`--only=<slug>`, `--min-gutter=<px>`, `--verbose`.

Like the permission matrix it is local only — it needs a live server and a password, so it is not
wired into CI. Same scratch-directory Playwright install, and the same reason to raise the rate
limits. Read `engineering-constraints.md` under "Browser-driven layout checks" before changing it;
a rendered box is not proof the user can see it, and four browser behaviours make naive checks
report defects that do not exist.

## Commands

```bash
cargo test
cargo clippy --locked --no-deps -- -D warnings
cargo fmt --all --check
```

Do not use `--workspace`; extension members are WASM-only. Use focused package/test filters while
iterating, then run the default-member suite before handoff.
