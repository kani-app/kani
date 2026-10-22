# Migration Policy

How Kani changes its database schema across releases, and what that requires of an operator.

## Forward-only

**Migrations are forward-only. Kani ships no down-migrations and will never generate one.**
A release applies every migration it carries that the database has not seen, in version order, at
startup. There is no supported path to move a database backwards.

The consequence is the rule that governs every upgrade: **the only rollback is a restore.** If a
new release misbehaves, you return to the previous binary *and* the backup taken before the
upgrade. Reverting the binary alone leaves a database whose schema is ahead of the code, which is
not a supported configuration and may fail in ways that look unrelated.

## Before upgrading

1. Take a backup — Settings → Storage, or the scheduled backup if configured.
2. Verify it: `kani-cli backup-verify <backup.zip>` checks the archive against the build you are
   running and refuses one written by a newer Kani. It verifies only; it restores nothing.
3. Note the running version. The support bundle records it, along with the schema version.

Backups are portable across builds in one direction: a backup restores onto a build at or newer
than the one that wrote it. Restoring onto an older build is refused, because a newer archive may
carry fields the older build would silently drop.

## How versions are tracked

Kani uses sqlx's migrator, so applied migrations live in the **`_sqlx_migrations`** table, keyed
by the numeric prefix of each file in `migrations/`. That table, not `PRAGMA user_version`, is
authoritative — `user_version` is not set and must not be relied on.

Two consequences:

- Migration files are **checksummed**. Editing an applied migration in place changes its checksum
  and the migrator refuses to start. `service/migration_checksums.rs` exists for the narrow case
  where a checksum must legitimately change, and it rewrites only known transitions.
- Applied migrations are effectively frozen. Fix a mistake with a new migration, never by editing
  an old one. This is also why applied migrations keep their original comments, whatever the
  current comment style says.

The support bundle reports the applied version as `db_schema_version` in `kani_info.json`, so a
bug report identifies the exact schema that produced it.

## Writing a migration

- One concern per file, named `<timestamp>_<snake_case_description>.sql`.
- Additive changes are safe: new tables, new nullable columns, new indexes.
- A column that drops or narrows data is a breaking change. Land it with the code that stops
  reading the old shape, never before.
- Run `cargo sqlx prepare --workspace -- --all-targets --tests` afterwards and commit `.sqlx/`.
  CI runs `cargo sqlx prepare --check` against a database built from `migrations/` alone, so a
  missing regeneration fails the build.
- Comment only a transformation or invariant the SQL cannot state. Do not restate identifiers.

## Version-skip upgrades

Upgrading across several releases at once is supported: the migrator applies every pending
migration in order. It is not separately tested for every pair of versions, so for a large jump,
take the backup seriously and check the logs at first start.

Downgrading is not supported at any distance.
