# API Stability

Kani publishes two compatibility tiers. A tier is a promise about change, not a statement about
quality: an unstable endpoint may be entirely reliable and still move without notice.

## What each tier promises

**Stable.** Within a major version, a stable operation may gain optional request fields and
optional response fields. It may not remove or repurpose an existing field, change a status code,
change a default, or move to another path. Breaking any of those requires a major version.

**Unstable.** No compatibility promise. The operation may change shape, move, or disappear in any
release, including a patch. Build against it only if you are prepared to track releases.

## REST API

Every operation in the OpenAPI document carries an `x-stability` extension whose value is
`stable` or `unstable`, so a generated client can read the tier directly rather than inferring it
from the path.

Unstable prefixes, and why:

| Prefix | Reason |
| --- | --- |
| `/rest/admin` | Server administration. Shape follows the admin UI's needs. |
| `/rest/ui` | Theme storage, introduced shortly before 1.0 and not yet exercised by third parties. |
| `/rest/jobs` | Background-job framework internals. |
| `/rest/server` | Process control, including restart. |
| `/rest/image_proxy` | Reader transport detail. |
| `/rest/boot_id`, `/rest/features`, `/rest/refresh` | Single-page-app plumbing for Kani's own frontend. |
| `/rest/trash` | Retention behaviour still settling. |

Everything else under `/rest` is stable. The default is deliberate: a new route is stable unless
someone lists it, so the reviewable mistake is forgetting to mark an internal route rather than
silently shipping an unannounced promise.

The single source of truth is `UNSTABLE_PREFIXES` in `kani-web/src/openapi.rs`.
`kani-web/tests/openapi_coverage_tests.rs` fails the build if an operation carries no tier, if an
administrative or internal route is marked stable, or if the tier is absent from the serialised
document.

## OPDS

`/opds` is **stable**. It implements OPDS 1.2 with the
[PSE page-streaming extension](http://vaemendis.net/opds-pse/2017), including `pse:count`,
`pse:lastRead`, OpenSearch 1.1 description, and Range-capable acquisition.

**It is deliberately absent from the OpenAPI document,** which is a statement about where the
contract lives, not about its tier:

- OPDS is self-describing. A reader discovers every feed by following links from the catalogue
  root, and `opds_client_tests.rs` proves that navigation by parsing the feeds and walking the
  links rather than asserting on strings.
- The authoritative schema is the OPDS and PSE specifications. Restating an external standard in
  our own document creates two definitions that can disagree.
- No OPDS reader consumes OpenAPI. Describing nine Atom endpoints there would add maintenance
  surface with no consumer.

The route set is instead pinned directly by `kani-web/tests/opds_stability_tests.rs`, which fails
if a stable OPDS path is removed or renamed. That is the same protection `x-stability` gives the
REST surface, applied where the contract actually is.

Page indexing follows Komga: PSE page numbers are 1-based, while stored progress remains a
0-based index. `opds_page_to_index()` is the single conversion point, and the
`opds_page_index_zero_based` setting exists for clients that expect the other convention.

## Command-line interface

`kani-cli` splits the same way. Stable covers the extension-authoring pipeline and the one tool
that works with no server at all:

```text
kani-cli new        kani-cli validate    kani-cli archive-verify
kani-cli generate   kani-cli build
```

`archive-verify` is stable because it re-hashes every file `ARCHIVE.json` claims and exits
non-zero on any mismatch, without Kani running. That makes it the executable half of the export
promise.

Every other subcommand is repository plumbing, a diagnostic, or an interface whose design is still
open, and its `--help` text is marked `[unstable]`:

| Subcommand | Reason |
| --- | --- |
| `setup`, `css`, `icons`, `lint` | Build this repository; not a supported interface. Keeping them unstable is what makes replacing them a later judgement call rather than a breaking change. |
| `keygen`, `publish`, `repo` | Complete and tested, but the extension-repository hosting model is not settled. Promote them as one group once an index is actually served. |
| `dsl`, `repl` | Print or manipulate internal representations such as the `Expr` AST, which is not a published schema. |
| `manifest`, `quality`, `probe`, `phash-compare` | Diagnostics. `ChapterManifest` is itself a frozen schema, but the command's presentation of it is not. |
| `backup-verify` | Reads the archive's `VERSION`, refuses a backup written by a newer build, prints entry counts, and directs the operator to Settings → Storage or `/rest/library/restore`. It restores nothing. Named `rollback` until 1.0; the old name is reserved for a command that actually rolls back, which needs the deferred `kani-cli` async restructure. |

The list is `STABLE_COMMANDS` in `kani-cli/src/commands/mod.rs`, checked against the help clap
renders by `kani-cli/tests/command_stability_tests.rs`.

## Extension-facing Rust surface

`kani-shared` is compiled into every extension, so its public items are an API for extension
authors even when nothing in this workspace calls them. They are **unstable**, but "no caller in
the workspace" is not on its own a reason to delete one.

Kept deliberately despite having no in-tree caller, because an extension author would reach for
them: the `Expr` DSL combinators `append_str`, `prepend_str`, and `get_key`; the preference
accessor `get_list`; `JsonHandle::object_iter`; and `parse_date` / `parse_date_rfc3339`, which
almost every source needs for chapter dates.

The test is whether a guest can reach the item and would want it. `HttpRequest::send_json` failed
both — it was gated behind `feature = "host"`, so no extension could call it, and no host code did
either. It was removed in the pre-1.0 sweep; `send_json_handle` covers the guest path.

## Changing a tier

Promoting unstable to stable is additive and may happen in a minor release. Demoting stable to
unstable is a breaking change and requires a major version. Record either in the changelog.
