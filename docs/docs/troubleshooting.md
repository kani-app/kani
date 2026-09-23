# Troubleshooting

Start with the server log, **Settings → Diagnostics**, **Settings → Source health**, and the jobs
page. Record the Kani commit or release, deployment type, and time of the failure before changing
state.

## Server does not become ready

Check `docker compose logs kani` or `journalctl -u kani`. Common causes are:

- The data or library directory is not writable by the service user.
- SQLite cannot create or lock `kani.db`, its WAL, or shared-memory sidecar.
- An explicit credential or proxy key is malformed or unreadable.
- The configured bind address is unavailable.
- A migration or startup subsystem reported a fatal error.

`/health` can succeed while `/ready` fails. Use readiness to decide whether the application can
serve traffic.

## Setup is unavailable

Setup is refused when an account already exists or the request is not from a loopback/private
address. Once the first account exists, sign in instead; setup cannot create a second
administrator.

For a remote new server, use a LAN connection or SSH tunnel. `KANI_ALLOW_REMOTE_SETUP=true`
temporarily disables the network restriction and lets the first reachable person claim the
server, so remove it immediately after use.

Behind a proxy, the address Kani sees depends on the deployment boundary. Do not broadly trust
forwarded client headers merely to make setup pass.

## Login or account recovery fails

- Check whether the account is active and whether failed-attempt lockout returned `Retry-After`.
- Confirm the browser sends cookies over the configured scheme; Secure cookies require HTTPS.
- For a state-changing API call using a cookie, include the `kani_csrf` value in `X-CSRF-Token`.
- Use a current TOTP code or unused backup code when step-up is required.
- Password reset requires working SMTP and an eligible account configuration.
- Review active sessions and the audit log after any suspicious login behavior.

Kani does not currently have a supported OIDC login callback. Setting an issuer variable is not a
fix for local-login failure.

## Database is locked

SQLite WAL does not make every filesystem safe. Stop external tools that hold the database open
and move `kani.db` off NFS, SMB, or object-backed filesystem adapters. Do not delete `-wal` or
`-shm` files from a live database.

If locks persist on a local disk, capture diagnostics and logs before restarting. Repeated forced
restarts can hide the process or job that holds the transaction.

## A source fails

1. Open **Settings → Source health** and identify load, circuit-breaker, browser, or network state.
2. Check whether the upstream site is reachable from the server.
3. Refresh the trusted repository and review an available update.
4. For browser-based sources, use **Test connection** in Settings → Advanced to confirm the solver
   supports capture scripts.
5. Test search, manga details, chapters, and pages independently.

An upstream layout change generally requires an extension update. Preserve the failing URL and a
small sanitized response fixture when reporting it; do not include cookies or source credentials.

## Downloads fail or remain queued

Inspect the download entry and its background job. A circuit-open source, rate limit, disk warning,
missing volume, locked file, or invalid page payload requires a different remedy.

Avoid repeatedly retrying the whole queue. Test one chapter after fixing the underlying issue.
Pending deletion is retried by tracked work; do not manually remove unrelated library paths.

## Images or pages do not load

- Distinguish a server-downloaded chapter from a remote-only chapter.
- Check the image proxy and source health rather than only the browser console.
- Confirm proxy body-size, timeout, and buffering configuration.
- Check disk capacity and manifest/integrity diagnostics.
- If browser offline data is stale, clear that chapter's offline cache and fetch it again.

## Stored credentials stop working after restore

Restoring `kani.db` without its matching `secret.key` prevents Kani from decrypting SMTP and tracker
credentials. Restore the matching key or re-enter every affected secret. Generating a new key does
not decrypt values written with the old one.

Similarly, restoring without `proxy.key` changes image-proxy signatures and can invalidate cached
URLs.

## Metrics returns 401

`/metrics` requires a general API token with `metrics:read`, sent as a bearer token. A session
cookie, OPDS token, or API token whose owner lost that permission is insufficient.

## Backup or restore fails

Check the backup job, destination permissions, free space, and encryption passphrase. A scheduled
path inside an unmounted container layer is not durable. Use `kani-cli backup-verify <backup.zip>` to
test whether the current build accepts an archive before attempting a production restore.

## Gather support information

1. Reproduce once with timestamps.
2. Export or copy the relevant diagnostics and bounded log window.
3. Record deployment, release/commit, source version, browser, and reverse proxy.
4. Remove tokens, cookies, credentials, private URLs, and personal library data.
5. Search [existing issues](https://github.com/kani-app/kani/issues) or open a
   [GitHub Discussion](https://github.com/kani-app/kani/discussions).

Use `RUST_LOG=kani=debug` only for a short reproduction and review the output before sharing it.
