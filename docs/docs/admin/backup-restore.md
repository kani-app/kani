# Backup and Restore

Kani has a logical backup archive, scheduled backup jobs, and ordinary filesystem backup. They
protect different data. Test restoration before relying on any one of them.

## What must be protected

| Data | Typical container path | Why it matters |
|---|---|---|
| SQLite database | `/data/kani.db` | Accounts, roles, settings, library metadata, progress, trust records, and job state |
| Credential key | `/data/secret.key` | Decrypts stored SMTP and tracker credentials |
| Proxy key | `/data/proxy.key` | Keeps signed image URLs valid across replacement |
| Installed extensions and browser state | under `/data` | Restores source execution and authenticated browser profiles |
| Library files | `/library` | Covers, downloaded pages, CBZ files, and manifests |

If explicit key environment variables or secret mounts are used, back up the external secret store
instead of expecting the key under `/data`.

## In-app backup

Under **Settings → Library**, export a Kani backup ZIP. The export can include chapter progress and
the logical application data represented in the preview. It is suitable for moving or merging a
library, but it is not automatically a byte-for-byte copy of every downloaded chapter.

An optional passphrase encrypts supported backup archives with ChaCha20-Poly1305. Store the
passphrase separately; Kani cannot recover it.

Before restoring, Kani previews archive metadata and available data groups. Choose merge and
component options deliberately. Missing sources are reported so they can be installed or resolved
later.

## Scheduled backups

**Settings → Storage** configures daily or weekly backup jobs, UTC execution time, destination,
retention count, and optional encryption. The destination must be writable by the server process.

In Docker, mount the destination as a persistent volume. A path inside the container's disposable
layer is lost when the container is replaced. Use **Run now** after saving a schedule, follow the
job from the jobs page, and restore its output on a disposable instance.

## Filesystem backup

For a full disaster-recovery copy, quiesce writes or use SQLite's online backup facility, then copy
the database, generated keys, extensions, and library data. Do not copy only `kani.db` while
ignoring an active WAL file and assume the result is consistent.

If the container includes the SQLite CLI, an online database copy can be made with `.backup`.
Otherwise use a host tool that supports SQLite online backup, or stop Kani cleanly before copying
the data volume.

## Restore a failed server

1. Stop Kani and preserve the failed data directory for investigation.
2. Restore `kani.db` and its matching key files into a clean writable data directory.
3. Restore installed extensions and library files, preserving paths recorded by the database.
4. Start the same Kani release that created the backup and check `/ready` and the logs.
5. Upgrade only after that restore is confirmed and the release's migration policy is understood.
6. Verify sources, stored credentials, representative downloads, and a chapter read before
   reopening the instance.

The `kani-cli backup-verify <backup.zip>` command verifies whether a Kani backup archive can be restored
by the current build; it does not replace the running database by itself.
