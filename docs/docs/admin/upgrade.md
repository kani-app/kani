# Upgrades

## Before upgrading

1. Read the target GitHub Release and changelog.
2. Confirm that its platform artifact or container coordinates match your deployment.
3. Create and test a backup with the current version.
4. Preserve `kani.db`, generated keys, installed extensions, and irreplaceable library files.
5. Record the current version and deployment configuration.

## Docker deployment

For a checkout-built image:

```bash
git fetch --tags
git checkout <release-tag>
docker compose up --build -d
docker compose logs -f kani
```

For a published image, follow the exact pull and signature-verification instructions attached to
that release. Do not assume `latest` identifies a stable or desired version.

Kani applies database migrations during startup. Wait for `/ready` before declaring the upgrade
healthy, then check diagnostics, sources, jobs, a representative manga, and a downloaded chapter.

## Binary deployment

Stop the service, install the verified target binary, and start it under the same user and data
directory. Keep the old binary until validation finishes, but do not run old and new binaries
against the same database concurrently.

## Rollback and migration policy

An older binary is not guaranteed to understand a database that a newer release migrated. Restore
the pre-upgrade backup when the release's rollback policy requires it; replacing only the binary
is not a safe general rollback procedure.

Compatibility and stability guarantees come from the release's migration policy. This page does
not invent guarantees for an unreleased or development build.

## Extensions

Server and extension versions are independent, but an extension can declare a minimum Kani version
and required host capabilities. Review source health after an upgrade and update extensions only
from a trusted repository. Keep the old artifact available until the new source loads and performs
a representative search and chapter fetch.
