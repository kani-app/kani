# Release Process

Kani develops on `develop`, stabilizes through pull requests, and updates `main` at release points.
Never commit a release directly to either permanent branch.

## Prepare

1. Merge feature work into `develop` and confirm unrelated work is not bundled into the release.
2. Resolve the target version from the release plan; do not copy a version from this page.
3. Update workspace package versions and the changelog consistently.
4. Regenerate SQLx metadata and any generated release configuration required by the changed
   manifests.
5. Run the default-member build, tests, clippy, formatting, docs, frontend checks, and extension
   build required by CI.
6. Review migration and stability notes and make breaking or rollback behavior explicit. A
   [migration squash](migrations.md#squashing-the-history) may only fold migrations this release's
   predecessors already shipped; folding an unreleased one strands every existing installation.
7. Open the release PR from `develop` to `main`.

## Rehearse artifacts

The release workflow runs on pull requests in upload rehearsal mode. Cargo Dist plans and builds
the configured native artifact matrix without creating a GitHub Release. The reusable Docker job
builds all configured architectures with publication disabled.

A pull request has no release tag, so rehearsal metadata must not collide with a real image tag.
Confirm the workflow's `publishing` output remains false and that Docker login, push, and signing
steps are skipped.

Rehearsal proves compilation and image construction. It cannot prove keyless container signing,
registry permissions, final tag metadata, or GitHub Release publication because those require a
tagged publishing run.

## Tag and publish

After the release PR merges and the exact commit is approved, create the semantic version tag
specified by the release plan. The tag workflow:

1. Generates the Cargo Dist plan.
2. Builds the configured platform archives and checksums.
3. Creates or updates the GitHub Release.
4. Invokes the reusable Docker workflow when configured.
5. Pushes and keyless-signs release images only on a publishing run.

Prerelease tags must remain prereleases and must not move a stable alias. Stable alias behavior,
registry coordinates, and the consumer verification command are release outputs; document them in
the release notes only after the publishing job proves them.

## Verify the release

- Download each published archive class and verify its checksum.
- Start at least one binary artifact with a disposable data directory.
- Pull the image by immutable release tag or digest and verify its signature using the exact
  release command.
- Complete first-run setup, check `/ready`, and exercise login, a source load, a job, and backup
  creation.
- Run `node scripts/verify-permission-matrix.mjs <base> <admin> <password>` against that instance.
  It is the only check on which UI surfaces each permission hides; the Rust permission tests cover
  the API contract, not visibility. It creates `permmatrix-*` accounts, so point it at the
  disposable instance rather than a real one.
- Confirm that release notes, supported targets, image coordinates, migration policy, and
  documentation describe the published artifacts.

If verification fails, do not repair a release by silently moving tags or replacing signed assets.
Follow the published correction or withdrawal policy.

## Documentation release

The docs workflow builds strictly and deploys versioned documentation from the configured release
branch with Mike. Every authored page must be in `mkdocs.yml`; a page missing from navigation can
otherwise build without being discoverable.
