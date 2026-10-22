#!/usr/bin/env python3
"""Fold the migration history into a single baseline file.

Applies every migration under `migrations/` to a throwaway SQLite database, dumps
the resulting schema plus the rows migrations seed, and writes that as the new
baseline. Emits the Rust `FOLDED_VERSIONS` list for
`kani-app/src/service/migration_checksums.rs`.

    python3 scripts/squash-migrations.py --cut 20260818000001 --baseline 20260818000002

Nothing is deleted: the old migration files are removed by hand once the generated
baseline has been diffed against a freshly migrated database.
"""

import argparse
import pathlib
import shutil
import sqlite3
import subprocess
import sys
import tempfile

# Tables SQLite maintains itself. `_sqlx_migrations` is rebuilt by sqlx, the FTS5
# shadow tables are recreated by their `CREATE VIRTUAL TABLE`, and emitting either
# as DDL would make the baseline fail to apply.
SKIP_TABLES = {"_sqlx_migrations", "sqlite_sequence"}
SHADOW_SUFFIXES = ("_config", "_content", "_data", "_docsize", "_idx")

# Tables whose contents are part of the schema contract rather than user data.
SEEDED_TABLES = ("roles", "role_permissions", "settings")

ROOT = pathlib.Path(__file__).resolve().parent.parent


def migrate(db_path, migrations_dir):
    result = subprocess.run(
        ["cargo", "sqlx", "migrate", "run", "--source", str(migrations_dir)],
        cwd=ROOT,
        env={**__import__("os").environ,
             "DATABASE_URL": f"sqlite:{db_path}?mode=rwc",
             "SQLX_OFFLINE": "false"},
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        sys.exit(f"sqlx migrate run failed:\n{result.stderr}")


def quote(value):
    if value is None:
        return "NULL"
    if isinstance(value, bytes):
        return "X'" + value.hex() + "'"
    if isinstance(value, (int, float)):
        return repr(value)
    return "'" + str(value).replace("'", "''") + "'"


def dump(db_path):
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    virtual = {
        row[0]
        for row in conn.execute(
            "SELECT name FROM sqlite_master "
            "WHERE type = 'table' AND sql LIKE 'CREATE VIRTUAL TABLE%'"
        )
    }
    shadow = {name + suffix for name in virtual for suffix in SHADOW_SUFFIXES}

    def skip(name):
        return name in SKIP_TABLES or name in shadow or name.startswith("sqlite_")

    rows = list(conn.execute("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY rowid"))
    statements = []
    for kind in ("table", "index", "trigger"):
        for row in rows:
            if row["type"] != kind or row["sql"] is None:
                continue
            if skip(row["name"]) or (kind != "trigger" and skip(row["tbl_name"])):
                continue
            statements.append(row["sql"].strip().rstrip(";") + ";")

    seeds = []
    for table in SEEDED_TABLES:
        columns = [info[1] for info in conn.execute(f"PRAGMA table_info({table})")]
        names = ", ".join(columns)
        for row in conn.execute(f"SELECT {names} FROM {table}"):
            values = ", ".join(quote(value) for value in row)
            seeds.append(f"INSERT INTO {table} ({names}) VALUES ({values});")

    # A migration that starts seeding a table absent from SEEDED_TABLES would be
    # dropped from the baseline silently, and no schema diff would show it.
    lost = []
    for row in rows:
        name = row["name"]
        if row["type"] != "table" or name in SEEDED_TABLES or skip(name):
            continue
        if conn.execute(f'SELECT EXISTS(SELECT 1 FROM "{name}")').fetchone()[0]:
            lost.append(name)
    if lost:
        sys.exit(
            "these tables hold rows the baseline would not reproduce: "
            + ", ".join(sorted(lost))
            + "\nAdd them to SEEDED_TABLES if migrations seed them."
        )

    versions = [row[0] for row in conn.execute(
        "SELECT version FROM _sqlx_migrations ORDER BY version")]
    return "\n\n".join(statements) + "\n\n" + "\n".join(seeds) + "\n", versions


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cut", type=int, required=True,
                        help="highest migration version to fold into the baseline")
    parser.add_argument("--baseline", type=int, required=True,
                        help="version for the generated baseline; must exceed --cut")
    parser.add_argument("--migrations", default=str(ROOT / "migrations"))
    args = parser.parse_args()
    if args.baseline <= args.cut:
        sys.exit("--baseline must sort after --cut")

    source = pathlib.Path(args.migrations)
    with tempfile.TemporaryDirectory() as work:
        work = pathlib.Path(work)
        staged = work / "migrations"
        staged.mkdir()
        for path in sorted(source.glob("*.sql")):
            if int(path.name.split("_", 1)[0]) <= args.cut:
                shutil.copy(path, staged / path.name)
        if not any(staged.iterdir()):
            sys.exit(f"no migrations at or below {args.cut}")

        db = work / "squash.db"
        migrate(db, staged)
        body, versions = dump(db)

    out = source / f"{args.baseline}_baseline.sql"
    header = (
        f"-- Generated squash of every migration through {args.cut}. Regenerate with\n"
        "-- `scripts/squash-migrations.py`; do not hand-edit, or the schema this produces\n"
        "-- stops matching the history it replaces.\n\n"
    )
    out.write_text(header + body)

    print(f"wrote {out} ({len(versions)} migrations folded)", file=sys.stderr)
    print(f"const BASELINE_VERSION: i64 = {args.baseline};")
    print("const FOLDED_VERSIONS: &[i64] = &[")
    for version in versions:
        print(f"    {version},")
    print("];")


if __name__ == "__main__":
    main()
