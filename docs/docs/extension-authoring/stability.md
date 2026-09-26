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
guest's generated bindings encode it — that is a 2.0 change. Every install, reload and startup
links the component against the host's imports before running it, so an extension that imports
something the host lacks (built for a newer Kani) is refused with the linker's error.

## Extraction DSL — `DSL_SCHEMA_VERSION`

`kani-shared::ast::DSL_SCHEMA_VERSION` is **6**, and the host reads every version from
`MIN_READABLE_DSL_SCHEMA_VERSION` (**5**) up to it. Every serialised `Blueprint` carries its
version as a postcard header. An extension's metadata records the version it was built with, and
the host checks it when the extension is installed, reloaded, and loaded at startup, as well as on
every decode:

```text
extension was built for blueprint schema version 7, but this Kani reads 5 to 6;
rebuild it with a matching kani-cli
```

**Rule for 1.x: a bump may only append.** postcard encodes enum variants by index, so appending an
`Expr` variant at the end of the enum leaves every older payload decodable, while inserting,
removing or reordering a variant or a field does not. A 1.x release may therefore raise
`DSL_SCHEMA_VERSION` for an appended variant (6 added `Expr::Arena` this way), and it keeps reading
every version from 5 onward. Raising the readable minimum, or changing an existing variant, is a
2.0 event.

An extension compiled by a newer `kani-cli` that uses a new variant needs a host that reads its
version; the install-time check refuses it on an older host with the message above rather than
failing on its first request.

## YAML schema — `schema_version`

`kani_yaml::yaml::schema::CURRENT_SCHEMA_VERSION` is **1**, and a YAML extension's
`schema_version` defaults to it when omitted. Validation rejects a file declaring a version
*newer* than the build understands:

```text
schema_version: 2 is newer than the schema version this kani-cli supports (1)
```

Older values are accepted. This surface is genuinely forward-compatible in the direction that
matters: raising `CURRENT_SCHEMA_VERSION` keeps every existing file valid, so a bump is additive
and permitted within 1.x.

A key the schema does not define is an error, not ignored, so a file using a key added in a later
release is refused by an older host with the key's name and line. Declare `min_kani_version`
alongside such a key so the refusal names the version it needs.

## Host version gate — `min_kani_version`

An extension may declare `min_kani_version` as a semver version. `kani-cli validate` rejects a
malformed value, and `kani_core::install_gating::check_min_kani_version` refuses installation on an
older host. Declare it whenever the extension uses a capability added after 1.0; that is the
supported way to depend on a newer host without breaking older ones.

## Summary

| Surface | Version | Check | Bump allowed in 1.x |
| --- | --- | --- | --- |
| WIT world | unversioned package | linked at install and load | Additive functions only |
| Extraction DSL | `DSL_SCHEMA_VERSION` = 6, reads 5–6 | readable range, at install and load | Yes, append-only; the minimum stays 5 |
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
