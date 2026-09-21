window.BENCHMARK_DATA = {
  "lastUpdate": 1789984530289,
  "repoUrl": "https://github.com/ArloB/kani",
  "entries": {
    "Benchmark": [
      {
        "commit": {
          "author": {
            "name": "Arlo Burke",
            "username": "ArloB",
            "email": "arlo.burke2@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "a1448d0fae1fe818161833a52effebb998b59ec0",
          "message": "ci: share one rust cache, drop the fat-LTO bench build, gate bench regressions (#3)\n\n* ci: share one rust cache, drop the fat-LTO bench build, gate bench regressions\n\n* docs: apply COMMENT_STYLE.md to the CI change",
          "timestamp": "2026-08-06T08:02:24Z",
          "url": "https://github.com/ArloB/kani/commit/a1448d0fae1fe818161833a52effebb998b59ec0"
        },
        "date": 1786026586382,
        "tool": "cargo",
        "benches": [
          {
            "name": "blueprint_eval/html_200_rows",
            "value": 3585107,
            "range": "± 9224",
            "unit": "ns/iter"
          },
          {
            "name": "blueprint_eval/json_200_rows",
            "value": 1176676,
            "range": "± 21778",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Arlo Burke",
            "username": "ArloB",
            "email": "arlo.burke2@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "d46c8093f1d6415a6485aec566f9d9f93cc4ecae",
          "message": "Feature/stage 4 tiering (#4)\n\n* feat: publish a compatibility tier for every REST operation and CLI command\n\n* feat: promote OPDS and the recovery commands to the stable tier\n\n* fix: rollback verifies a backup, it does not roll one back\n\n* refactor: rename kani-cli rollback to backup-verify\n\n* docs: add the Stage 4 stability statements and report the schema version\n\n* feat: emit API token timestamps as RFC 3339\n\n* chore: remove EXAMPLE_EXTENSION.yaml and correct the built-in sources claim\n\n* chore: stop naming individual extensions across the repo\n\n* fix: resolve client IP through one trusted-proxy aware path\n\n* feat: accept WASM metadata flags when publishing\n\n* feat: expand source compatibility and image transforms\n\n* style: apply repository formatting\n\n* Changed to a single version docs flow",
          "timestamp": "2026-08-07T14:18:44Z",
          "url": "https://github.com/ArloB/kani/commit/d46c8093f1d6415a6485aec566f9d9f93cc4ecae"
        },
        "date": 1786942453384,
        "tool": "cargo",
        "benches": [
          {
            "name": "blueprint_eval/html_200_rows",
            "value": 3621212,
            "range": "± 226139",
            "unit": "ns/iter"
          },
          {
            "name": "blueprint_eval/json_200_rows",
            "value": 1199736,
            "range": "± 10552",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Arlo Burke",
            "username": "ArloB",
            "email": "arlo.burke2@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "d46c8093f1d6415a6485aec566f9d9f93cc4ecae",
          "message": "Feature/stage 4 tiering (#4)\n\n* feat: publish a compatibility tier for every REST operation and CLI command\n\n* feat: promote OPDS and the recovery commands to the stable tier\n\n* fix: rollback verifies a backup, it does not roll one back\n\n* refactor: rename kani-cli rollback to backup-verify\n\n* docs: add the Stage 4 stability statements and report the schema version\n\n* feat: emit API token timestamps as RFC 3339\n\n* chore: remove EXAMPLE_EXTENSION.yaml and correct the built-in sources claim\n\n* chore: stop naming individual extensions across the repo\n\n* fix: resolve client IP through one trusted-proxy aware path\n\n* feat: accept WASM metadata flags when publishing\n\n* feat: expand source compatibility and image transforms\n\n* style: apply repository formatting\n\n* Changed to a single version docs flow",
          "timestamp": "2026-08-07T14:18:44Z",
          "url": "https://github.com/ArloB/kani/commit/d46c8093f1d6415a6485aec566f9d9f93cc4ecae"
        },
        "date": 1787547428719,
        "tool": "cargo",
        "benches": [
          {
            "name": "blueprint_eval/html_200_rows",
            "value": 3698762,
            "range": "± 323032",
            "unit": "ns/iter"
          },
          {
            "name": "blueprint_eval/json_200_rows",
            "value": 1196519,
            "range": "± 22619",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Arlo Burke",
            "username": "ArloB",
            "email": "arlo.burke2@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "d46c8093f1d6415a6485aec566f9d9f93cc4ecae",
          "message": "Feature/stage 4 tiering (#4)\n\n* feat: publish a compatibility tier for every REST operation and CLI command\n\n* feat: promote OPDS and the recovery commands to the stable tier\n\n* fix: rollback verifies a backup, it does not roll one back\n\n* refactor: rename kani-cli rollback to backup-verify\n\n* docs: add the Stage 4 stability statements and report the schema version\n\n* feat: emit API token timestamps as RFC 3339\n\n* chore: remove EXAMPLE_EXTENSION.yaml and correct the built-in sources claim\n\n* chore: stop naming individual extensions across the repo\n\n* fix: resolve client IP through one trusted-proxy aware path\n\n* feat: accept WASM metadata flags when publishing\n\n* feat: expand source compatibility and image transforms\n\n* style: apply repository formatting\n\n* Changed to a single version docs flow",
          "timestamp": "2026-08-07T14:18:44Z",
          "url": "https://github.com/ArloB/kani/commit/d46c8093f1d6415a6485aec566f9d9f93cc4ecae"
        },
        "date": 1788173325560,
        "tool": "cargo",
        "benches": [
          {
            "name": "blueprint_eval/html_200_rows",
            "value": 3712425,
            "range": "± 9733",
            "unit": "ns/iter"
          },
          {
            "name": "blueprint_eval/json_200_rows",
            "value": 1209730,
            "range": "± 36713",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Arlo Burke",
            "username": "ArloB",
            "email": "arlo.burke2@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "df66fb2fcd24f333d1719211a008732abb9c9c2b",
          "message": "v1.0.0-rc.1 (#18)\n\n* Improved CSP hash mechanism + added missing vendor error\n\n* Changes to DSL parser to avoid overflows during parsing\n\n* Started work on browser_shim improvements\n\n* Improve DSL arena safety and browser challenge resilience\n\n* Added missing auto_scroll field to test fixtures\n\n* Capture browser payloads in the solver, with capability probing and auth\n\n* Retire the local Puppeteer browser and the capture_url_param ABI\n\n* Gate browser_payload on the live solver at install time\n\n* Name the missing capability instead of referring back to it\n\n* Warn when solver traffic would cross a routable network in the clear\n\n* Map source failures to statuses the caller can act on\n\n* Fix two tests that failed only under parallel load\n\n* Let an admin test the solver URL before saving it\n\n* Detect duplicates by page identity, not archive bytes\n\n* Retire the browser env vars, counters, and diagnostics\n\n* Record that two browser settings no longer govern anything\n\n* Stop offering two browser settings that do nothing\n\n* Drop the browser from the image and document the solver\n\n* Reuse one HTTP client for solver calls\n\n* Refresh the sqlx cache against the migrated schema\n\n* Add the detectors the waste sweep needs\n\n* Rebaseline the i18n orphan list against the pre-1.0 branch\n\n* Teach the dead-JS scanner about dynamic-import member access\n\n* Close the five escalations from the waste-sweep triage\n\n* Make endpoint tests assert what the handler produced\n\n* Replace 98 per-route auth tests with one route-driven harness\n\n* Consolidate the repetitive half of the REST test suite\n\n* Fold four families of near-identical tests into tables\n\n* Make two rejection tests assert why they were rejected\n\n* Make 31 WASM tests actually run, and fix weak assertions outside kani-web\n\n* Stop the chapter-list stream bridge losing items to a partial write\n\n* Tighten the remaining crates' rejection and filter tests\n\n* Verify covers reach the library, not just the manga row\n\n* Delete the dead frontend the detectors identified\n\n* Verify JS by bundling rather than node --check\n\n* Rename the browser settings for the worker they actually configure\n\n* Consolidate 113 migrations into one baseline\n\n* Fail the squash generator on rows it would drop\n\n* Constrain host-side HTML pagination\n\n* Remove unused dependencies and dead Rust, rename the V8 reaper\n\n* Keep the stub metadata provider out of release builds\n\n* Match the HAR entry to the endpoint under test\n\n* Share one artifact signing implementation\n\n* Share the evaluator field and id helpers\n\n* Share the codegen option emitter\n\n* Fix two lint failures the verification had been hiding\n\n* Narrow 383 pub items to the visibility they actually need\n\n* Wire the two unfinished features, delete the six dead items\n\n* Drain redundant_pub_crate by narrowing, not widening\n\n* Collapse the duplicate Codecov config and drop dead config\n\n* Clear the bundle output directory before each build\n\n* Walk the operator through solver setup\n\n* Add the support-bundle and extension-authoring quickstart docs\n\n* Bump workspace version to 1.0.0-rc.1\n\n* Removed multiple-blank-lines issue from support-bundle.md\n\n* modified:   docs/docs/admin/support-bundle.md\n\n* Fixed clippy error\n\n* Restore .github/workflows/fragments/build-setup.yml\n\n* Record the build-setup.yml false-dead-code finding in the constraints register",
          "timestamp": "2026-09-02T15:02:27Z",
          "url": "https://github.com/ArloB/kani/commit/df66fb2fcd24f333d1719211a008732abb9c9c2b"
        },
        "date": 1788772551517,
        "tool": "cargo",
        "benches": [
          {
            "name": "blueprint_eval/html_200_rows",
            "value": 3653829,
            "range": "± 44588",
            "unit": "ns/iter"
          },
          {
            "name": "blueprint_eval/json_200_rows",
            "value": 1190816,
            "range": "± 83642",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Arlo Burke",
            "username": "ArloB",
            "email": "arlo.burke2@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "2b3372ca5a99e84be15d70e854bca9b58c5e43da",
          "message": "Merge pull request #21 from ArloB/release/1.0.0-rc.3\n\nRelease/1.0.0 rc.3",
          "timestamp": "2026-09-14T07:11:30Z",
          "url": "https://github.com/ArloB/kani/commit/2b3372ca5a99e84be15d70e854bca9b58c5e43da"
        },
        "date": 1789379306811,
        "tool": "cargo",
        "benches": [
          {
            "name": "blueprint_eval/html_200_rows",
            "value": 2847538,
            "range": "± 6448",
            "unit": "ns/iter"
          },
          {
            "name": "blueprint_eval/json_200_rows",
            "value": 914295,
            "range": "± 2222",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Arlo Burke",
            "username": "ArloB",
            "email": "arlo.burke2@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "9045c8482fdf15e80c8e2e6f6a8e309544054f49",
          "message": "Ci/release setup dist profile (#22)\n\nImproved the release pipeline by reducing build time",
          "timestamp": "2026-09-14T14:49:14Z",
          "url": "https://github.com/ArloB/kani/commit/9045c8482fdf15e80c8e2e6f6a8e309544054f49"
        },
        "date": 1789984529216,
        "tool": "cargo",
        "benches": [
          {
            "name": "blueprint_eval/html_200_rows",
            "value": 3691988,
            "range": "± 98237",
            "unit": "ns/iter"
          },
          {
            "name": "blueprint_eval/json_200_rows",
            "value": 1242720,
            "range": "± 19681",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}