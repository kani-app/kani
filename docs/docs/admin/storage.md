# Storage

## Data and library roots

| Location | Contents | Backup priority |
|---|---|---|
| Data directory | SQLite database, generated keys, installed extensions, caches, and browser profiles | Critical |
| Library directory | Covers, chapter payloads, CBZ archives, and manifests | High, but some source-backed content may be recoverable |

The Docker image uses `/data` and `/library`. Binary installations use `KANI_DATA_DIR` and the
library setting or `KANI_LIBRARY_DIR` override.

## Volumes

**Settings → Storage** can define and inspect storage volumes. Use volumes to place different data
on appropriate disks and review free space before moving or importing a large library. Paths are
resolved on the server, not in the administrator's browser.

Changing a path does not make existing files teleport. Use Kani's supported path migration flow
and review its preview. A target that matches no files or collides with existing data is rejected.

## SQLite

Kani uses SQLite in WAL mode with separate read capacity and serialized writes. Keep `kani.db` on a
local filesystem with correct POSIX-style locking. Network filesystems frequently cause `database
is locked` errors or corruption risks.

Do not edit the database while Kani is running. Use application APIs, backup/restore, and supported
migrations.

## Disk pressure

The maintenance settings include a disk-warning threshold, trash retention, audit retention, and
thumbnail formats. Diagnostics shows current storage state. Treat a warning as a capacity event:
pause large downloads or exports, clear supported disposable data, and add capacity before the
filesystem reaches zero free bytes.

Deleting a manga moves it to trash. Purge through **Settings → Trash** so database and filesystem
state remain consistent. Chapter downloads can be deleted independently while preserving metadata
and progress.

## Object storage

Kani currently uses filesystem paths and does not provide a general S3-compatible library backend.
Mounting object storage through a filesystem adapter does not automatically make its consistency
and locking semantics safe for SQLite or library mutation.
