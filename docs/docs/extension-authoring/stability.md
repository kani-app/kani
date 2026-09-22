# Extension ABI Stability

This page states what an extension author can rely on across a Kani 1.x release, and what a
2.0 would be allowed to change. It covers four independently versioned surfaces: the WIT world,
the extraction DSL, the YAML schema, and the host version gate.

## The 1.x promise

Within 1.x, the extension interface is **additive only**. Kani may add WIT functions, records,
enum cases and YAML keys; it may not remove one, change a signature, change the meaning of an
existing field, or tighten validation on input that previously passed.

An extension built against 1.0 must load and run on every later 1.x host without recompiling.
The converse does not hold: an extension using a function added in 1.4 will not load on 1.2, which
is what `min_kani_version` exists to express.

## WIT world

The world is `kani-extension` in package `kani:extension`
([`kani-core/wit/kani.wit`](https://github.com/kani-app/kani/blob/develop/kani-core/wit/kani.wit)).
The host imports `http`, `html`, `json`, `utility`, `prefs`, `extraction`, `cache` and
`scripting`; the guest exports `manga-provider`.

The package carries **no explicit WIT version annotation**. Compatibility is enforced by the two
version numbers below plus `min_kani_version`, not by WIT package resolution. Adding an explicit
`@major.minor.patch` to the package is a candidate for 2.0: it regenerates every binding and would
have to land with the codegen change, so it is not worth doing inside 1.x for a mechanism that is
already covered.

Adding a WIT function is additive and safe. Changing an existing signature is not, because the
guest's generated bindings encode it — that is a 2.0 change.

## Extraction DSL — `DSL_SCHEMA_VERSION`

`kani-shared::ast::DSL_SCHEMA_VERSION` is **5**. Every serialised `Blueprint` carries it as a
postcard header, and the host checks it on arrival:

```rust
if version != kani_shared::ast::DSL_SCHEMA_VERSION {
    return Err(format!(
        "Blueprint DSL schema version {} is not supported (host requires {}); \
         recompile the extension", …));
}
```

**The check is strict equality, not a minimum.** A host accepts exactly one DSL version, so
bumping it rejects every extension compiled against the previous value with a "recompile the
extension" error. That makes a bump a breaking change for the entire ecosystem at once.

**Rule for 1.x: `DSL_SCHEMA_VERSION` does not change.** It bumps only for a change to the
`Expr`/`Blueprint` *wire shape*, which is therefore a 2.0 event. Adding an `Expr` variant is not
automatically such a change — postcard encodes enum variants by index, so appending a variant at
the end of the enum is compatible with hosts that never receive it, while inserting or reordering
one is not. Append only.

## YAML schema — `schema_version`

`kani_yaml::yaml::schema::CURRENT_SCHEMA_VERSION` is **1**, and a YAML extension's
`schema_version` defaults to it when omitted. Validation rejects a file declaring a version
*newer* than the build understands:

```text
schema_version: 2 is newer than the schema version this kani-cli supports (1)
```

Older values are accepted. This surface is genuinely forward-compatible in the direction that
matters: raising `CURRENT_SCHEMA_VERSION` keeps every existing file valid, so a bump is additive
and permitted within 1.x. Use it when adding keys that older builds must not silently ignore.

## Host version gate — `min_kani_version`

An extension may declare `min_kani_version` as a semver version. `kani-cli validate` rejects a
malformed value, and `kani_app::install_gating::check_min_kani_version` refuses installation on an
older host. Declare it whenever the extension uses a capability added after 1.0; that is the
supported way to depend on a newer host without breaking older ones.

## Summary

| Surface | Version | Check | Bump allowed in 1.x |
| --- | --- | --- | --- |
| WIT world | unversioned package | binding generation | Additive functions only |
| Extraction DSL | `DSL_SCHEMA_VERSION` = 5 | strict equality | **No** — breaks every extension |
| YAML schema | `CURRENT_SCHEMA_VERSION` = 1 | rejects newer only | Yes, additive |
| Host gate | `min_kani_version` | semver at install | N/A, per extension |

## Deprecating something in 2.0

1. Announce in the release notes of a 1.x release, with the replacement available in the same
   release.
2. Keep the old surface working for the remainder of 1.x. A deprecated WIT function stays
   callable; a deprecated YAML key keeps validating.
3. Remove only in 2.0, and list every removal in the migration notes.

Extensions in the official repository are rebuilt against the new world before a 2.0 tag, so the
`kani-fixture-source` conformance suite (`kani-core/tests/wasm_conformance_tests.rs`) is the gate
that proves a change is really additive.
