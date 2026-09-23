# Extension Authoring Quickstart

This walkthrough creates a declarative YAML source, records one real response as a local fixture,
tests extraction against that fixture, and builds a WASM component. Run the commands from the Kani
repository; if `kani-cli` is installed separately, replace `cargo run -p kani-cli --` with
`kani-cli`.

The YAML workflow is the shortest route to a new source. `kani-cli new my-source --rust` can
instead scaffold a Rust/WASM crate, but that is the advanced path covered by
[Rust extensions](rust-extensions.md).

## 1. Scaffold the extension

```bash
cargo run -p kani-cli -- new my-source
```

This creates `my-source.yaml`. Open it and set the source identity, base URL, endpoint routes, and
extraction fields. Keep the [YAML schema](yaml-schema.md) and [DSL grammar](dsl-grammar.md) nearby
as references rather than trying to define the whole source at once.

The [`kani-example`](https://github.com/kani-app/kani/tree/develop/kani-extensions/kani-example)
development extension is a useful working implementation to read alongside this walkthrough.

## 2. Validate the YAML

```bash
cargo run -p kani-cli -- validate my-source.yaml
```

Validation checks the schema, required endpoint fields, routes, and DSL expressions before a live
request or build. Fix all reported errors before continuing.

## 3. Record a HAR fixture

Record a real upstream response once so extraction can be tested repeatedly without contacting the
site on every run:

```bash
cargo run -p kani-cli -- repl record my-source.yaml popular --output popular.har
```

The endpoint may be `popular`, `search`, `manga_details`, `chapter_list`, or `pages`. Add trailing
`key=value` arguments for route placeholders and query parameters:

```bash
cargo run -p kani-cli -- repl record my-source.yaml chapter_list manga_id=abc page=1 --output chapters.har
```

HAR fixtures contain upstream response data. Review them for personal or sensitive information
before committing or sharing them.

## 4. Test extraction locally

Run the endpoint against the fixture and assert its expected row count:

```bash
cargo run -p kani-cli -- repl test my-source.yaml popular.har popular 20
```

The arguments are the YAML file, HAR file, endpoint, and expected row count. The test normally
chooses the successful HAR entry whose URL matches the endpoint route. If the route does not appear
directly in the recorded URL, identify the entry with a URL fragment:

```bash
cargo run -p kani-cli -- repl test my-source.yaml popular.har popular 20 --url-contains /catalog
```

For focused debugging, inspect the extension structure or explain how a single DSL expression is
parsed:

```bash
cargo run -p kani-cli -- repl inspect my-source.yaml
cargo run -p kani-cli -- repl explain 'self.first("h2").text().trim()'
```

See [DSL grammar](dsl-grammar.md) for extraction behavior and [Rhai hooks](rhai-hooks.md) when a
source needs bounded request or response scripting.

## 5. Generate the Rust crate

```bash
cargo run -p kani-cli -- generate my-source.yaml
```

Generation compiles the validated declaration into a generated Rust crate. Continue editing the
YAML and regenerate; do not maintain the generated source as a second implementation. Use
`--force` when intentionally replacing an existing generated crate.

## 6. Build the WASM component

```bash
cargo run -p kani-cli -- build kani-my-source
```

The build compiles and optimizes the extension, then writes its component to `wasm_sources/`. To
build Kani's development and test extensions, including `kani-example`, run:

```bash
cargo run -p kani-cli -- build --dev
```

You now have a working `.wasm` component. Before relying on a host feature, check
[ABI stability](stability.md). When the extension is ready to distribute, continue with
[Publishing and distribution](publishing.md) for signing and repository setup.
