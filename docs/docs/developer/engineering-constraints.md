# Engineering Constraints

This register holds durable implementation knowledge that is useful during development but too
large, historical, or cross-cutting to live in source comments. Revalidate an entry when its named
tool, dependency, workflow, or subsystem changes.

## Frontend and CSS

### Tailwind layer order can override authored selectors

**Constraint.** A selector in Tailwind's utilities layer wins over the same selector in Kani's
authored component layer because layer order is considered before specificity. Authored component
selectors must not reuse Tailwind utility names.

**Evidence.** The `.list-item` collision was encountered in four separate UI changes before the
component was renamed `.li-row`. The generated stylesheet placed the utility after the authored
rule even when the authored selector appeared more specific during local inspection.

**Consequence.** Layout declarations can disappear without an invalid rule or an obvious cascade
warning.

**Enforcement.** Keep authored component names semantic and Kani-specific. Inspect the generated
stylesheet and computed styles when a declaration is present in source but absent at runtime.

**Revalidate when.** Tailwind, its standalone CLI, or the CSS layer structure changes.

## Container builds

### cargo-chef preserves the workspace linker configuration

**Constraint.** `cargo chef cook` receives `.cargo/config.toml`, whose target configuration invokes
`scripts/fast-linker.sh`. The linker script must therefore be copied into the builder before the
cook step.

**Failure signature.** When the script is absent, foundational build dependencies such as
`proc-macro2`, `quote`, `serde`, and `libc` fail to link together. The errors look like unrelated
crate failures rather than a missing workspace script.

**Enforcement.** The Docker builder copies `scripts/fast-linker.sh` before dependency cooking, and
the production-image workflow builds the same Dockerfile.

**Revalidate when.** The Docker stages, cargo-chef, `.cargo/config.toml`, or linker selection changes.

### The Kani image ships no browser

**Constraint.** Browser capture runs in the solver container. The Kani image installs neither
Chromium nor `puppeteer-core`, and has no build argument to add them.

**Evidence.** Building the runtime base with and without the browser package set measured 1.44 GB
against 260 MB on 2026-08-12, so carrying them cost 1.18 GB. The Kani image built without them on
the same day is 340 MB and contains no `chromium` binary. The solver image is 1.07 GB, but it is
already required for any source behind a managed challenge, so it is not additional.

**Consequence.** A deployment that wants browser sources runs two containers. A deployment that
does not is 1.18 GB smaller than before, and cannot run browser sources at all.

**Enforcement.** The runtime stage installs `nodejs` for the sandbox worker and nothing browser
related. Browser capture reaches the solver over HTTP or fails with a solver-specific error.

**Revalidate when.** The solver protocol changes, a source needs a browser capability the solver
cannot provide, or the image base changes.

### Managed-challenge capture stays in the solving browser

**Constraint.** The challenge solve, host-page token generation, extension script, and payload
capture must run in one solver browser session. Clearance is never carried to a second browser.

**Evidence.** Cloudflare documents clearance as bound to the visitor and device. Against a live
browser-gated source on 2026-08-12, across 20 captures per cell: a fresh solver took 12.57 s at
p50, and a cleared session 2.07 s, with no correctness failures in 80 captures and no re-challenge
in 40. The solve is almost all of the cold cost — 10.4 s of it — while the capture itself runs
about 300 ms either way. On an identical local fixture the solver beat local Puppeteer at every
percentile, 246 ms against 370 ms at p50, so removing the local browser cost no latency.

**Consequence.** Cookie fidelity cannot make cross-browser replay reliable. A solver without
scripted capture can still solve ordinary HTTP requests, but protected browser sources may fail.
The solver executes extension-authored JavaScript and must remain private.

**Enforcement.** `kani.capture/2` installs the script before page code, rearms the idle deadline on
`resetPayloadTimer`, caps payload size, serialises each deterministic per-source/domain session,
and removes injected scripts after every capture. Kani memoises challenged routes, reaps sessions
with source browser state, and keeps runtime browser-disable controls as hard gates.

**Revalidate when.** Cloudflare clearance behavior, the solver protocol, browser-source security
controls, session lifecycle, or the browser source set changes.

### The V8 settings are named for the worker they configure

**Constraint.** `v8_max_memory_mb`, `v8_idle_timeout_s` and `v8_debug_logging` govern the Node/V8
worker that runs source scripts. They were named `browser_*` for the Puppeteer pool that browser
capture used before it moved into the solver, and that pool no longer exists.

**Evidence.** `browser_max_instances` reached `V8Config::max_instances`, which only ever appeared
in `browser_stats()` and the diagnostics payload; nothing enforced a limit, so it was dropped
rather than renamed. `browser_idle_timeout_s` was live — `jobs/v8_reap.rs` reads it and
`jobs/recurring.rs` sets the reap cadence from it — but it was governing two unrelated lifetimes
at once: the local V8 worker and the solver's own sessions.

**Consequence.** The solver already expires sessions on `SOLVER_SESSION_TTL_MINUTES`, so an
operator lowering the setting could only make the host reap earlier than the solver, never later,
and the two could disagree silently. `reap_solver_sessions` now takes `http::solver_session_ttl()`
and the setting governs the local worker alone.

**Enforcement.** Migration `20260818000001` renames the three columns and drops the fourth. The
DTO carries `#[serde(alias = "browser_*")]` on each renamed field, because they are required
rather than optional and a backup written before the rename would otherwise fail to deserialise —
`backup_import_tests::a_backup_written_before_the_v8_rename_still_restores` fails with
`missing field v8_debug_logging` if an alias is removed.

**Revalidate when.** A solver-side equivalent of either control is exposed and needs a home, or
the local V8 worker stops being the thing these configure.

## HTTP routing

### Static source capabilities must coexist with the parameterized route

**Constraint.** `/sources/capabilities` must resolve to the bulk handler while
`/sources/{id}/capabilities` continues to resolve per-source capabilities. Router composition must
not interpret the literal `capabilities` segment as a source ID.

**Failure signature.** The bulk endpoint returns `400` or `404` because the parameterized handler
attempts to parse `capabilities` as an ID.

**Enforcement.** `bulk_route_is_not_swallowed_by_the_per_source_route` exercises both endpoints in
the same router.

**Revalidate when.** Axum, route syntax, or source-router composition changes.

### Interactive navigation requires rate-limit burst headroom

**Constraint.** The global request limiter must accommodate a normal authenticated navigation
burst while still enforcing its sustained rate over time.

**Evidence.** A library page generated 21 REST calls and a manga page generated 24, arriving at
about seven calls per second while each page settled. A login followed by eight ordinary page
navigations generated 148 calls. A previous configuration interpreted 50 requests per second as
one request every 50 seconds, making it roughly ten times stricter than intended.

**Enforcement.** The limiter uses an explicit per-second rate and a burst allowance sized above
the measured navigation sequence.

**Revalidate when.** Frontend request fan-out, startup fetching, or rate-limiter configuration
changes.

## Scan and search limits

### Scans release the write connection during source I/O

**Constraint.** Chapter pages are collected before opening the single-connection write
transaction. The transaction performs one batch write after all source requests finish.

**Evidence.** Holding the write transaction across page fetches, retries, backoff, and challenge
handling blocked a recurring job for 28.8 seconds during a library scan.

**Enforcement.** The scan workflow separates collection from `insert_chapters_batch` and refreshes
page counts within the final transaction.

**Revalidate when.** Write-pool sizing, scan collection, source retry behavior, or chapter batch
insertion changes.

### Barren-page tolerance is relevant only to page-granular sources

**Constraint.** A scan stops after three consecutive pages containing only known chapters unless
the source ends pagination first. Each additional tolerance unit costs at most one request per
manga and scan.

**Evidence.** Sources that return a complete chapter list in one response never reach the guard at
all — a roughly 200-chapter series measured against two such sources completed in two requests.
Page-granular sources can hide new chapters beyond a run of known pages, so lowering the value
risks omissions.

**Enforcement.** `scan_barren_page_tolerance` remains runtime-configurable with a default of three.

**Revalidate when.** Source pagination behavior or the chapter-scan stopping rule changes.

### Global search retains measured timeout headroom

**Constraint.** One source receives six seconds before global search returns the other sources'
results. Operators can raise the value for slow links or challenge-heavy sources.

**Evidence.** Thirty anonymous searches across five installed sources measured a 0.46-second
median, 1.05-second p90, and 1.46-second maximum. A Cloudflare-blocked source failed in 0.47
seconds. Six seconds retains roughly four times the slowest observed latency.

**Enforcement.** `global_search_timeout_secs` is runtime-configurable and defaults to six.

**Revalidate when.** The source set, challenge handling, transport behavior, or global-search
fan-out changes.

### Tracker synchronization spaces calls per access token

**Constraint.** Tracker synchronization spaces calls for the same access token by 700 milliseconds
and extends that delay when a provider returns `Retry-After`.

**Evidence.** AniList documents a normal limit of 90 requests per minute and response headers for
remaining quota and retry timing. Its documentation also warns that incident limits may be reduced;
as of 2026-08-06 it reports a temporary 30-request-per-minute degraded limit. See the
[AniList rate-limit contract](https://docs.anilist.co/guide/rate-limiting).

**Consequence.** A single global throttle needlessly couples users, while ignoring provider backoff
causes repeated failed syncs. Static spacing alone cannot guarantee compliance during a degraded
provider limit.

**Enforcement.** `TokenThrottle` keys spacing and backoff by access token, and tracker responses feed
`Retry-After` into its backoff window. Each recurring run also has a bounded entry count.

**Revalidate when.** Provider quotas, tracker response handling, batch size, scheduling frequency,
or per-token throttle behavior changes.

## Delivery workflows

### `dist-workspace.toml` can reference a workflow fragment with no `uses:` site

**Constraint.** `github-build-setup = "fragments/build-setup.yml"` in `dist-workspace.toml` has
`cargo-dist` read that file directly as an asset when it generates or validates `release.yml`
(the `dist generate --check` job in `ci.yml`). A grep for `uses:` across `.github/workflows/`
does not find this reference, because it is consumed as a config-key file path, not invoked
through GitHub Actions' own reference syntax.

**Failure signature.** `dist generate --check` fails with `failed to load github-build-setup
file … No such file or directory`, on a file that has zero conventional workflow references and
looks safe to delete by every ordinary check.

**Consequence.** `.github/workflows/fragments/build-setup.yml` was deleted as dead CI config
during the 2026-08/09 waste sweep on the (true but incomplete) claim that it had no `uses:`
reference from any workflow. Restored once CI caught it on the next PR.

**Enforcement.** Before deleting anything under `.github/workflows/fragments/`, grep
`dist-workspace.toml` for its filename in addition to grepping workflows for `uses:`, and run
`dist generate --check` locally.

**Revalidate when.** `dist-workspace.toml`'s `github-build-setup` key changes, or the fragment
directory gains a second consumer.

### Content-hashed bundle output accumulates unless the directory is cleared

**Constraint.** `build_js` clears `static/js/dist` before invoking esbuild. The bundler names
split chunks by content hash, so a rebuild writes new files beside the old ones instead of
replacing them, and `stage_assets_for_embedding` copies whatever the directory holds.

**Evidence.** Before the clean was added the directory held 483 files and 8.0 MB, of which
408 files and 7.09 MB — 88% by size — were unreachable from the `app.js` entry point and were
being embedded in every release binary. Ten hashed copies of each page module had accumulated.
A clean build produces 71 files and 906 KB.

**Consequence.** Any future output whose filename varies by content needs the same treatment.
Reachability is the test, not file count: walk the import graph from the entry the HTML names.

### Matrix outputs cannot carry per-architecture digests

**Constraint.** A GitHub Actions matrix exposes one last-writer-wins job output rather than an
independent value for each matrix member. Per-architecture image digests must cross the job
boundary as artifacts.

**Consequence.** Using one matrix job output silently loses one architecture and produces an
incomplete manifest.

**Enforcement.** Each image build uploads a digest artifact; the merge job downloads both named
artifacts, rejects a missing digest, creates the multi-architecture manifest, and signs its digest.

**Revalidate when.** GitHub Actions changes matrix-output semantics or the Docker workflow stops
using separate architecture jobs.

### rust-cache keys on Cargo environment variables

**Constraint.** `Swatinem/rust-cache` folds `CARGO_*` and `RUST*` environment variables into its
cache key, so `shared-key` alone does not make two jobs share a cache. Every job intended to share
one must declare an identical environment block and toolchain setup.

**Evidence.** Setting the profile-debug variables per job produced three separate caches for one
lockfile: `v0-rust-ci-Linux-x64-{db7c195c,df7e546d,9be4e61c}-e650112a`, written by the cache
warmer, the lint job and the test job respectively. The 1.5 GB warm cache on `develop` was
unreachable from either pull-request job, and a lint run that normally takes 27 minutes took
37 minutes 51 seconds.

**Consequence.** Cache misses are silent. Jobs still succeed, so the only visible symptom is a
build time that looks like a cold runner.

**Enforcement.** The environment block and `dtolnay/rust-toolchain` inputs are identical in
`ci.yml`'s lint and test jobs and in `cache-warmer.yml`, and are declared at workflow level so a
job cannot override them locally.

**Revalidate when.** rust-cache changes its key derivation, or a job adds a `CARGO_*`/`RUST*`
variable.

### Compiling benchmarks under the bench profile is not a compile check

**Constraint.** `[profile.bench]` inherits `[profile.release]`, so `cargo bench --no-run` rebuilds
the whole dependency tree at `opt-level = 3` with fat LTO and `codegen-units = 1`. Answering "do
the benchmarks still compile" does not need that; `cargo check --benches` answers it in the dev
profile and reuses the clippy build.

**Evidence.** The step accounted for 988 seconds of a 27-minute lint job, against 253 seconds for
clippy itself.

**Consequence.** `cargo check` does not catch link-time or monomorphisation-time failures. The
weekly benchmark workflow still performs a real bench-profile build, which is where those surface.

**Enforcement.** `ci.yml` runs `cargo check --locked --benches`; `bench.yml` builds and runs the
benchmarks for real.

**Revalidate when.** A benchmark target starts depending on link-time behaviour, or the bench
profile stops inheriting release.

### Criterion reports a regression only against stored history

**Constraint.** Criterion compares a run against a baseline under `target/criterion`, and
rust-cache prunes non-dependency artifacts from `target/` before saving. A benchmark workflow that
keeps its history only in the build directory has no baseline and cannot detect a regression.

**Evidence.** Every scheduled run reported no previous run and uploaded an artifact that nothing
compared. Scheduled runs additionally check out the default branch, so the workflow measured
`main` while development continued on `develop`.

**Consequence.** The benchmark degrades into a compile-and-execute smoke test while appearing to
guard performance.

**Enforcement.** `bench.yml` emits libtest lines via `--output-format bencher`, feeds
`benchmark-action/github-action-benchmark`, stores history on the `benchmarks` orphan branch
because `gh-pages` belongs to mike, fails past a 150% regression, and names its target branch
explicitly rather than relying on the default-branch checkout.

**Revalidate when.** The benchmark set changes, the alert threshold proves too noisy for a shared
runner, or documentation deployment stops using `gh-pages`.

## Component-model streams

### A stream write transfers only what the reader takes

**Constraint.** `StreamWriter::write` transfers as many items as the reader accepts and returns the
remainder in its buffer; `StreamResult::Complete(n)` reports the count written, not that the whole
batch went. Guest code writing a batch must use `write_all`, which retries until the buffer drains
and returns values only once the reader has dropped.

**Evidence.** `bridge_chapter_list_stream` wrote a page per call as `let (result, _buf) =
tx.write(items)`, matched `Complete(_)` as success, and discarded `_buf`. Against a host consumer
reading one item per poll, a two-chapter page delivered its first chapter and dropped the second:
draining `paginated-stream` from `kani-test-abi` yielded `["p1-1", "p2-1"]` instead of
`["p1-1", "p1-2", "p2-1", "p2-2"]`.

**Consequence.** Any extension relying on the default `get-chapter-list-stream` bridge silently
lost chapters, at a rate set by how the host drained the stream. Production polls per page and does
not use the bridge, so no released behaviour depended on it.

**Enforcement.** `kani-core/tests/wasm_abi.rs::abi_get_chapter_list_stream_bridge_delivers_all_pages_in_order`
asserts the exact ids and their order, so a batch that loses items fails rather than returning
fewer. The test needs `wasm_sources/test-abi.wasm`, which CI builds in the `test` job.

**Revalidate when.** wit-bindgen changes `write_all`'s contract, or a guest starts writing batches
through `write` directly.

## Storage and recovery

### A squashed baseline cannot be stamped onto a partial history

**Constraint.** Baseline adoption replaces a pre-squash `_sqlx_migrations` history with one
baseline row only when the recorded set is exactly the folded set. Any other state — a missing
folded version, an unrecognised version, a failed row — is refused with the history left intact.

**Evidence.** Stamping the baseline records that the database is at the baseline's schema. A
database missing one folded migration is missing that migration's schema change, and no later
migration would reapply it, so the claim would be silently false and every subsequent query
against the absent column would fail at runtime rather than at startup.

**Consequence.** The squash cut point is bounded by deployment, not by convenience: every
installation that must be upgradable has to have applied every migration being folded in. The
20260818000002 baseline could fold the entire history only because no tagged release existed, so
no installation was stranded. A later squash must cut at the last released migration.

### Migration checksums record two unrelated kinds of drift

**Constraint.** `TRANSITIONS` maps a legacy checksum to a current one for a migration edited in
place, and is guarded by a semantic hash proving the edit was comment-only. Baseline adoption is
checksum-blind and matches on version and success alone.

**Evidence.** The two mechanisms answer different questions. A transition asserts that a file's
bytes changed while its effect did not, which requires comparing effects. Adoption discards rows
describing migrations whose files no longer exist, so their checksums have nothing left to
describe.

**Consequence.** Adoption runs before reconciliation and must not depend on it. Coupling them
would make a legacy checksum on a folded migration block an upgrade for no reason.

### Lease coordination uses one atomic word for modelability

**Constraint.** The draining flag and active-lease count share one atomic word, and acquisition
uses compare-and-exchange while the draining bit is clear.

**Evidence.** Splitting the state across two atomics creates a StoreLoad ordering problem requiring
a global sequentially consistent order. Loom treats sequential consistency as acquire/release and
reports a false positive for that design, while it models one atomic location's modification order
precisely. Keeping `kani-lease` as a leaf crate also avoids applying Loom's global configuration to
Tokio dependencies.

**Consequence.** Separate draining and count atomics either weaken the algorithm or make the valid
algorithm impossible to verify with the project's model checker.

**Enforcement.** Run `RUSTFLAGS="--cfg loom" cargo test -p kani-lease`; the model covers acquisition,
release, and drain interleavings.

**Revalidate when.** Lease state representation, memory ordering, source hot-swapping, Loom, or the
crate dependency graph changes.

### JPEG encoder estimates require a noise margin

**Constraint.** Upgrade comparison treats encoder-quality differences below twelve points as
equivalent.

**Evidence.** Inversion of libjpeg quantisation-table scaling was tested against JPEGs produced by
the `image` crate and differed from the requested quality by approximately eight points. Encoder
table choices and rounding make the estimate useful for ordering, not as an absolute quality value.

**Consequence.** Comparing raw estimates without headroom would report ordinary encoder variation
as an upgrade or downgrade.

**Enforcement.** `ENCODER_MARGIN` gates the encoder axis in quality comparison tests.

**Revalidate when.** JPEG parsing, quantisation-table inversion, fixture encoders, or upgrade-axis
ranking changes.

### Chapter upgrades preserve the replaced archive

**Constraint.** Applying an upgrade moves the held CBZ into `.replaced/` before clearing chapter
metadata and queuing the replacement download. A cross-filesystem move falls back to copy-then-
remove only after the copy succeeds.

**Consequence.** Deleting the held archive directly would make an upgrade irreversible if the new
download failed or proved worse.

**Enforcement.** Upgrade integration tests require the old archive to survive in `.replaced/` and
verify its retention purge separately.

**Revalidate when.** Upgrade application, library paths, download replacement, trash retention, or
recovery behavior changes.

## Lints

### Whole-group pedantic and nursery are not worth their volume here

**Constraint.** `[workspace.lints.clippy]` enables named lints that find dead or redundant code,
not the `pedantic` or `nursery` groups. A lint sits at `allow` only while its recorded violation
count is being drained.

**Evidence.** Enabling both groups produced 3,295 warnings across the workspace. The largest
categories were 535 missing `# Errors` doc sections, 517 `Self` repetitions, and 471 `must_use`
suggestions — none of which identify unused or duplicated code. Restricting to the waste-finding
subset produced 397, of which 303 were `redundant_pub_crate`.

**Consequence.** CI runs `cargo clippy -- -D warnings`, so a group that mostly reports style would
have made every build fail on documentation preferences.

**Enforcement.** The block lists lints individually with a count beside each `allow`, so the
backlog is visible in the manifest rather than tracked elsewhere.

**Revalidate when.** A group's contents change materially, or the `allow` counts reach zero and the
remaining lints can be promoted.

## Test execution

### Environment mutation in tests is not serialised

**Constraint.** `std::env::set_var` and `std::env::remove_var` require that no other thread access
the environment for the duration of the call. The requirement is process-wide, not per-key, so
using disjoint keys does not satisfy it. Rust's default test harness runs tests concurrently.

**Evidence.** The `kani-web` library test binary mutates the environment from two tests on
different keys, `KANI_PROXY_SECRET` in `proxy.rs` and `KANI_DATA_DIR` in `auth.rs`, with no
serialisation between them. `kani-cli` mutates the environment only through one helper in
`build.rs`, which passes a distinct key per test.

**Consequence.** The mutation is unsound under the threaded harness rather than merely racy on a
shared key. No failure has been observed, so the exposure is latent.

**Enforcement.** Each unsafe block carries a `SAFETY:` note stating the requirement and the local
facts that bound it, rather than asserting single-threadedness the harness does not provide.

**Revalidate when.** A third environment-mutating test appears, a test starts reading the
environment concurrently, or the crates adopt a serialisation guard.

## Browser-driven layout checks

### A rendered box is not proof the user can see it

**Constraint.** Four browser behaviours make `getBoundingClientRect` disagree with what is on
screen. Content inside a closed `<details>` keeps a non-zero rect and is not `display: none`. A box
clipped by an `overflow: hidden` ancestor still measures at full size. Playwright's `fullPage`
capture is sized to `documentElement.scrollWidth`, so a document wider than its viewport yields an
image with dead space even when nothing scrolls. And this app scrolls inside
`div.flex-1.min-w-0[overflow-y:auto]`, not on the document, so `fullPage` captures one screenful
and `document.scrollingElement` is the wrong thing to scroll or to test for end-of-list.

**Evidence.** The 2026-09 mobile audit produced four confident false findings from these: a 379px
`<pre>` inside a collapsed disclosure reported as 362px of content under the bottom nav; a
sources-health page reported as horizontally broken when `body.scrollWidth` was correct and only
`documentElement.scrollWidth` differed; nine controls reported as unreachable because the probe
settled `scrollers[0]` rather than each element's own scroller; and lazy lists reporting
`atEnd: true` while still mid-list for the same reason.

**Consequence.** A layout check that trusts a rect alone reports defects that do not exist, and
misses real ones by settling the wrong container. Each false finding cost a round of investigation.

**Enforcement.** `scripts/verify-mobile-layout.mjs` excludes `details:not([open])` and
overflow-clipped boxes, resolves each element's own scrollable ancestor before asserting
end-of-scroll, and asserts horizontal scrolling by moving `scrollLeft` rather than comparing
widths.

**Revalidate when.** The scroll container moves out of `div.flex-1.min-w-0`, or Playwright changes
how `fullPage` sizes a capture.

### Long-press cannot be driven with synthetic mouse events

**Constraint.** The select-mode handlers in `static/js/pages/library.js` and
`components/virtual-chapter-list.js` start a 400ms timer on `pointerdown` and cancel it on
`pointermove`. Playwright's `page.mouse` emits a move, so the timer never fires and select mode
cannot be entered.

**Evidence.** Repeated attempts to enter bulk selection with `mouse.move`/`down`/`up` produced no
bulk bar; a CDP `Input.dispatchTouchEvent` sequence held 800ms with no movement entered it on both
the library grid and a chapter row.

**Consequence.** Any check covering bulk selection must use CDP touch events. A mouse-driven
attempt fails silently and looks like the feature being absent.

**Revalidate when.** The long-press threshold or its cancel conditions change.
