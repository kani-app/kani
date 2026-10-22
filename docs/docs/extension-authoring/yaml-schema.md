# YAML Extension Schema

Kani's declarative format describes source metadata, requests, extraction, filters, preferences,
and optional sandboxed scripts. `kani-yaml` owns parsing and validation; `kani-cli` scaffolds,
validates, inspects, generates, and builds it.

## Start an extension

```bash
cargo run -p kani-cli -- new my-source
cargo run -p kani-cli -- validate my-source.yaml
cargo run -p kani-cli -- repl inspect my-source.yaml
```

A compact HTML source looks like this:

```yaml
id: my-source
name: My Source
version: "0.1.0"
base_url: "https://example.com"
language: en

endpoints:
  search:
    route: "/search"
    queries:
      q: "$query$"
      page: "$page$"
    container: ".manga-list .item"
    fields:
      id: 'self.first("a").attr("href").split("/").at(-1)'
      title: 'self.first("h3").text().trim()'
      cover_url:
        expr: 'self.first("img").attr("src").resolve_url("https://example.com")'
        optional: true

  manga_details:
    route: "/manga/$manga_id$"
    container: ":root"
    fields:
      id: '"$manga_id$"'
      title: 'dom("h1").text().trim()'
      status: 'dom(".status").text().trim().lower().fallback("unknown")'

  chapter_list:
    route: "/manga/$manga_id$"
    container: ".chapter-list a"
    fields:
      id: 'self.attr("href").split("/").at(-1)'
      number: 'self.text().capture("([0-9.]+)").at(1).parse_float()'
      language: '"en"'
    has_next_page: false

  pages:
    route: "/chapter/$chapter_id$"
    container: ".reader img"
    fields:
      index: "index()"
      url: 'self.attr("src").resolve_url("https://example.com")'
```

Validate examples rather than treating this page as a substitute for the parser.

## Top-level fields

| Field | Required | Purpose |
|---|---|---|
| `id` | yes | Lowercase identifier matching `[a-z][a-z0-9-]*` |
| `name` | yes | Display name |
| `version` | yes | Semantic version |
| `base_url` | yes | Absolute source origin |
| `language` | no | Language code, default `en` |
| `nsfw` | no | Content-rating declaration |
| `unrestricted_http` | no | Permit requests beyond the normal host restriction |
| `schema_version` | no | Declarative schema compatibility level |
| `min_kani_version` | no | Minimum host semantic version |
| `requires_capabilities` | no | Host capabilities required at install time |
| `metadata` | no | Description, icon, languages, sections, and rate limit |
| `endpoints` | effectively | Provider operations implemented by the source |
| `filters` / `option_sets` | no | Search and browse controls |
| `preferences` | no | Installation-specific configuration |
| `id_encoding` | no | Pack and unpack composite manga or chapter IDs |
| `cache` | no | Named cache namespaces |
| `chapter_sort` | no | Source-supported chapter ordering |
| `factory` | no | Expand one template into several sources |
| `browser_scripts` | no | JavaScript payload capture in the solver's browser |
| `scripts`, `pre_request`, `on_status` | no | Sandboxed Rhai logic |

## Metadata and rate limits

`metadata.icon` accepts a base64 PNG or WebP image up to 64 KiB decoded; SVG and other markup
are refused.
`metadata.languages` can advertise more than the primary language. Sections describe named source
views.

```yaml
metadata:
  description: "Example catalogue"
  rate_limit:
    rps: 2.0
    burst: 8
    max_concurrent: 4
    max_hook_requests: 3
  languages: [en, ja]
  sections:
    - id: latest
      name: Latest
```

The host enforces these limits around extension requests. Do not set them above what the upstream
service permits.

## Endpoints

The available endpoint keys are `popular`, `search`, `manga_details`, `chapter_list`, and `pages`.
Each common endpoint can declare:

- `route`, HTTP `method`, `headers`, and `queries`.
- `type: html` or `type: json`.
- A `container`, document `bindings`, row `fields`, and document `scalars`.
- `has_next_page`, `total_pages`, and source-native `pagination` where applicable.
- Document-level `then` or row-level `for_each` sub-fetches.
- Browser payload capture or request/response hooks.

Routes, headers, and queries interpolate `$query$`, `$page$`, `$page_size$`, `$manga_id$`,
`$chapter_id$`, `$pref:key$`, and declared composite-ID fields where the endpoint makes them
available.

A field is either a DSL string or an object with `expr` and `optional`. Required provider fields
are validated: manga details need `id`, `title`, and `status`; chapters need `id`, `number`, and
`language`; pages need `index` and `url`.

`popular` may be a full endpoint or delegate to another endpoint:

```yaml
endpoints:
  popular:
    delegate_to: search
    empty_without_filters: true
```

## HTML and JSON extraction

HTML containers and DOM methods use CSS selectors. JSON navigation uses RFC 6901 JSON Pointers:

```yaml
endpoints:
  search:
    route: "/api/search"
    type: json
    container: "/data"
    fields:
      id: 'self.ptr("/id").str()'
      title: 'self.ptr("/attributes/title").get(pref("language")).str().fallback("Unknown")'
```

See [DSL grammar](dsl-grammar.md) for expression types and null behavior.

## Pagination and chaining

Use `$page$` and `$page_size$` directly when the upstream accepts them. For fixed chunks, declare
`pagination` with `native_page_size`, `offset_param`, and an `offset_type` of `item`, `page`, or
`cursor`. Cursor pagination also declares the JSON Pointer that yields the next token.

`then` performs one sub-fetch for the document. `for_each` performs a bounded sub-fetch per row.
Each step names another declared endpoint, a URL expression, `merge_as`, concurrency, and failure
behavior. Keep per-row fan-out small and within the source rate limit.

## Filters and preferences

Filters support checkbox, select, text, sort, multiselect, integer-range, and date-range controls.
An endpoint maps filter IDs to query parameters and can configure boolean, array, and omission
formatting. Reusable option sets may be static or fetched and cached.

Preferences support toggle, select, text, and multi-value-list kinds. Text preferences can be
marked secret. Access them with `pref("key")` in the DSL or `$pref:key$` in request templates.

## Composite IDs, cache, and factories

`id_encoding` declares named fields, delimiter, and base64-url, base64, passthrough, or hex
encoding. Field maps build an encoded ID; `$manga.field$` and `$chapter.field$` unpack it for later
requests.

Cache declarations specify namespace scope, TTL, entry limit, and an optional key template. Rhai
hooks can read and write declared namespaces through `ctx.cache`.

A `factory` contains several source identities and dot-path overrides. Building the template
validates each expansion and emits one extension per source. An `overrides` path must already exist
in the template; new entries go under `add`, so a misspelt path fails the build.

## Browser endpoints

`via: browser_payload` loads `page_url` in the solver's browser and runs a named `browser_scripts` entry in that
page before its own scripts. The entry must call `passPayload`; long-running captures can call
`resetPayloadTimer` after each unit of progress. When a Kani-compatible FlareSolverr is configured,
managed challenges are solved and captured in its browser without transferring clearance to a
second browser. The interpreted YAML backend supports extracting the returned payload. Browser
support must be enabled at runtime. Prefer direct HTTP extraction when possible.

## Build and inspect

```bash
cargo run -p kani-cli -- validate my-source.yaml
cargo run -p kani-cli -- generate --force my-source.yaml
cargo run -p kani-cli -- build kani-my-source
cargo run -p kani-cli -- repl inspect my-source.yaml
cargo run -p kani-cli -- repl test my-source.yaml
```

`repl test` and `repl replay` pick the HAR entry whose URL matches the endpoint's route. A HAR
written by `repl record` holds a single entry, but one exported from a browser holds every request
the page made, so an endpoint with no matching entry is an error rather than a guess. Pass
`--url-contains <fragment>` when the route does not appear in the recorded URL.

Use `kani-cli --help` and subcommand help for the current flags. Generated Rust is an artifact of
the YAML definition; edit the YAML and regenerate rather than maintaining both by hand.
