# Downloads and Exports

## Download queue

Queue a chapter from its manga page, queue several with bulk actions, or let a manga's download
rules submit newly discovered chapters. The **Downloads** page shows active work and history,
including progress, failures, retry, cancellation, and deletion of local data.

The server enforces global and per-source concurrency. A source can also declare its own rate
limit. Raising concurrency does not bypass the upstream site's rate limit and can make failures
more frequent.

Canceling an active download asks the background job to stop. Deleting a completed download
removes the local chapter payload but keeps the chapter metadata and reading progress.

## Failures and retries

Transient network and rate-limit failures may be retried by the job framework. A final failure is
recorded in download history and diagnostics. Check the source health page before repeatedly
retrying every failed chapter.

If a locked or unavailable file leaves a download pending deletion, Kani schedules a tracked
retry.

## Export formats

Downloaded chapters can be exported as CBZ, EPUB, and KEPUB where their manifest and files are
complete. Kindle-oriented MOBI/AZW3 export requires Kindle Comic Converter in the server image;
the standard Dockerfile includes it only when built with `INSTALL_KCC=true`.

Exports are generated from server-side chapter data. Large exports can take time and may be
submitted as background jobs. Keep proxy timeouts and response-size limits in mind.

## Archive quality and integrity

Kani records page manifests and quality data for downloaded archives. Operators can run scrub and
maintenance jobs to detect missing or inconsistent files. The CLI also includes archive and CBZ
inspection commands for advanced diagnosis.

Exports are convenience copies, not full server backups. Use [Backup and restore](../admin/backup-restore.md)
to preserve accounts, settings, repositories, and library metadata.
