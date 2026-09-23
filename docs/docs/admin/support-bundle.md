# Support Bundle

A support bundle packages the information commonly needed to investigate a Kani problem into one
zip file. An administrator can download it from the web interface and attach it to a support
request.

## What the bundle contains

The zip contains six files:

| File | Contents |
|---|---|
| `kani_info.json` | Kani version, Git commit SHA, process uptime, applied database schema version, operating system, and architecture |
| `config.json` | The complete Kani settings document, with sensitive string values redacted |
| `db_schema.sql` | `CREATE TABLE` and `CREATE INDEX` statements from SQLite, without database rows |
| `extensions.json` | The installed extensions reported by diagnostics |
| `diagnostics.json` | The same runtime diagnostics returned by `GET /rest/admin/diagnostics` |
| `logs.jsonl` | Recent structured log entries, with one JSON object per line |

The `db_schema_version` in `kani_info.json` is the latest successfully applied SQLx migration. It
helps maintainers identify the exact schema that produced a report.

## Redaction and privacy

In `config.json`, string values are replaced with `***REDACTED***` when their field name contains
`secret`, `token`, `password`, `key`, or `dsn`, matched case-insensitively. This redaction is
recursive through nested objects and arrays.

The database schema and log content are not redacted. The schema contains no row data, but it can
reveal table and index names. Logs can contain source URLs, manga titles, usernames, paths,
hostnames, and other operational details. Extension metadata and non-secret settings may also be
private. Review the contents of every bundle before sharing it and remove anything you do not want
to disclose.

## Download a bundle

1. Open **Settings → Diagnostics**.
2. Select **Download support bundle**.
3. Wait for Kani to prepare and download the timestamped zip file.

Downloading a bundle requires an authenticated account with the **ServerManage** permission. The
underlying endpoint is `GET /rest/admin/support-bundle`; it returns `401` for an unauthenticated
request and `403` when the account lacks permission.

## Share it with a report

First search the [existing Kani issues](https://github.com/kani-app/kani/issues). If the problem has
not been reported, follow the repository's current support convention: open a
[GitHub Discussion](https://github.com/kani-app/kani/discussions), describe how to reproduce the
problem, and attach the reviewed bundle. The team can direct confirmed bugs into an issue.
