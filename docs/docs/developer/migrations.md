# Migrations

Schema changes live in `migrations/` and run at startup through
`kani-app/src/service/migration_checksums.rs`, which wraps `sqlx`'s migrator with two
reconciliation steps. Both exist because `sqlx` identifies a migration by a checksum over its
file, so any edit to a file an installation has already applied is a fatal version mismatch.

## Adding a migration

Create `migrations/<UTC timestamp>_<description>.sql` and write forward-only SQL. There are no
down migrations: a mistake is corrected by a further migration, never by editing the file that
shipped it.

Regenerate the offline query cache afterwards, against a database built from migrations alone:

```sh
DATABASE_URL="sqlite:$(mktemp -u).db?mode=rwc" SQLX_OFFLINE=false cargo sqlx migrate run
cargo sqlx prepare --workspace -- --all-targets --tests
```

Never hand-edit `.sqlx/`. The `pre-push` hook runs `prepare --check` against a fresh database and
rejects a stale cache.

## Editing a migration that has already shipped

Only comment-only edits are permitted, and only through the `TRANSITIONS` table.

Add an entry recording the version, the checksum installations currently carry (`legacy`), the
checksum the edited file now hashes to (`current`), and `semantic` — a SHA-384 over the file with
comments and redundant whitespace stripped. `validate_transitions` recomputes the semantic hash at
startup and refuses to run if it does not match, so an entry cannot smuggle an executable change
past the checksum rewrite. `reconcile` then updates `_sqlx_migrations` in place before the migrator
compares anything.

An entry that names a version no longer present in `migrations/` is an error, not a no-op. Squashing
therefore removes the entries for every migration it folds in.

## Squashing the history

A squash replaces the accumulated files with one baseline. Run the generator, which applies the
existing history to a throwaway database and dumps the result:

```sh
python3 scripts/squash-migrations.py --cut <last folded version> --baseline <new version>
```

It prints the `BASELINE_VERSION` and `FOLDED_VERSIONS` constants to paste into
`migration_checksums.rs`. Delete the folded files and the `TRANSITIONS` entries naming them, then
regenerate `.sqlx`.

The baseline is generated, not written. It reproduces the schema, the indexes, the triggers, and
the rows migrations seed — `roles`, `role_permissions`, and the `settings` singleton. It omits
`_sqlx_migrations`, `sqlite_sequence`, and the FTS5 shadow tables, all of which are rebuilt by the
statements that remain.

**Verify by diffing databases, not by reading the file.** Build one database from the old history
and one from the baseline alone, then compare `sqlite_master`, every table's `PRAGMA table_info`,
the seeded rows, and the FTS5 shadow tables. The shadow tables are the check that matters: they
prove `CREATE VIRTUAL TABLE` regenerated what the dump deliberately left out.

**The baseline version must sort after every folded version and must never have been applied
anywhere.** Reusing a historical version would leave `sqlx` comparing the baseline's checksum
against a row that records a different migration.

### What adoption does to an existing database

An installation that predates the baseline carries one `_sqlx_migrations` row per folded migration.
`adopt_baseline` replaces all of them with a single baseline row, matching on version and success
only — the checksums record migrations that no longer exist, so they carry no meaning.

Adoption demands the folded set exactly. A database missing any folded migration, carrying a
version the baseline does not know, or recording a failed migration is refused with an error naming
the version, and its history is left untouched. Stamping the baseline onto such a database would
claim a schema it does not have.

This is why the cut point matters: **every supported installation must already have applied every
migration being folded in.** Cutting past a released version strands that release with no upgrade
path.
