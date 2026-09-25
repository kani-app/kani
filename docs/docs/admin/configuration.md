# Configuration

Kani separates deployment configuration from runtime settings:

- Environment variables control secrets, directories, network binding, process limits, and
  startup overrides.
- The settings UI controls behavior that an administrator may tune while the server is running.

Do not create a `kani.toml`; Kani does not read one.

## Directories and networking

| Variable | Default | Purpose |
|---|---|---|
| `KANI_BIND` | `0.0.0.0:8242` | Listen address and port |
| `KANI_DATA_DIR` | process working directory | Location of `kani.db` and generated keys |
| `KANI_LIBRARY_DIR` | stored setting | Startup override for the library directory |
| `KANI_STATIC_DIR` | *(unset)* | Serve the frontend from this directory instead of the copy embedded in the binary |
| `KANI_CORS_ORIGIN` | request origin | Allowed browser origin |
| `KANI_SECURE_COOKIES` | `false` | Mark session cookies Secure |
| `KANI_TRUSTED_PROXIES` | *(none)* | Comma-separated addresses or CIDR blocks permitted to set `X-Forwarded-For` |

The container sets `KANI_DATA_DIR` through its working directory, serves static files from
`/app/static`, and sets the library to `/library`.

## Keys and secrets

| Variable | Format | Behavior when absent |
|---|---|---|
| `KANI_SECRET_KEY_FILE` | Path containing a 64-character hex key | Falls through to the inline or generated key |
| `KANI_SECRET_KEY` | 64-character hex key | A key is generated in `KANI_DATA_DIR/secret.key` |
| `KANI_PROXY_SECRET` | Base64url-encoded 32 bytes | A key is generated in `KANI_DATA_DIR/proxy.key` |

The credential key encrypts stored SMTP and tracker credentials. The proxy key signs image-proxy
URLs. Mount secret files read-only where possible, never print key values in support output, and
back up generated key files with the database.

## Authentication and exposure

| Variable | Purpose |
|---|---|
| `KANI_ALLOW_REMOTE_SETUP` | Permit first-account setup outside loopback/private networks |
| `KANI_ALLOW_REGISTRATION` | Seed the registration setting at startup |
| `KANI_PUBLIC_INSTANCE` | Force the hardened internet-facing profile |
| `KANI_SESSION_TIMEOUT_SECONDS` | Seed session inactivity timeout; applies after restart |
| `KANI_MAX_LOGIN_ATTEMPTS` | Seed per-identity failed-login threshold |
| `KANI_MAX_IP_ATTEMPTS` | Seed per-address failed-login threshold |
| `KANI_LOGIN_LOCKOUT_SECONDS` | Seed the lockout window |
| `KANI_API_RATE_PER_SECOND` / `KANI_API_BURST_SIZE` | Sustained API requests per second, and the burst allowance above it |
| `KANI_PROXY_RATE_PER_SECOND` / `KANI_PROXY_BURST_SIZE` | The same pair for the image proxy, which the reader drives far harder |

Use the Security settings page for the live database-backed values. Environment overrides are
primarily for initial provisioning and managed deployments.

## Sources and browser support

| Variable | Purpose |
|---|---|
| `KANI_SOURCE_INSTALL_ALLOWED` | Disable all new upload, URL, and repository installs |
| `KANI_OFFICIAL_REPO_URL` | Bootstrap an operator-selected signed repository |
| `KANI_OFFICIAL_REPO_KEY` | Override its compiled or configured Ed25519 public key |
| `KANI_BROWSER_PROFILES_DIR` | Location of legacy browser profiles, removed when a source is deleted |
| `KANI_SOLVER_SECRET` | Shared key sent to the solver; must match its `API_KEY` |
| `KANI_RHAI_MAX_OPS` | Rhai operation budget |
| `KANI_RHAI_MAX_STRING` | Rhai string-size budget |
| `KANI_RHAI_MAX_ARRAY` | Rhai array-size budget |

Raising scripting budgets weakens a resource limit applied to untrusted extension logic. Prefer
rewriting the extension unless a measured workload requires the change.

### Browser sources and the solver

For browser-backed sources behind a managed challenge, run `ghcr.io/kani-app/flaresolverr`, Kani's
fork of FlareSolverr. It solves the challenge and runs the extension's capture script in the same browser, then reuses
one cleared session per source and domain. A stock FlareSolverr remains supported for ordinary
HTTP challenge solving and best-effort cookie replay, but cannot reliably capture device-bound
pages. Each source's browser toggle disables its browser endpoints.

**Pin a release.** Each release is published as `v<upstream version>-kani.<n>`, for example
`v3.5.0-kani.1`, and the compose file uses one. `latest` follows the fork's main branch, so an
unpinned solver can change under a running deployment. Upgrade by changing the tag deliberately;
the fork's changelog lists what each release adds.

**The fork keeps its browser off your network.** Pages the solver loads, and every script and
subresource they fetch, are refused if they resolve to a private, loopback or cloud-metadata
address. A stock FlareSolverr has no such guard, so Kani raises a **Browser solver** warning in
**Settings → Diagnostics** while the configured solver lacks it.

The `solver` service in `docker-compose.yml` already runs this image. To set it up:

1. Start both services. The compose file does not publish the solver's port to the host at all —
   only the `kani` container can reach it, over the compose network's service DNS. `Test
   connection` (step 3) works the same way, since it's a server-side call from `kani-web`, not
   your browser.
2. In **Settings → Advanced**, set the solver URL to `http://solver:8191/v1` — `solver` is the
   compose service name. If you've uncommented the `ports:` block to debug it directly from the
   host instead, use `http://127.0.0.1:8191/v1` there.
3. Press **Test connection**. It reports *Browser sources supported* for this image, or
   *Challenge solving only* for a stock FlareSolverr. The same state appears under
   **Diagnostics → Browser runtime**.
4. If something other than Kani needs to reach the solver, set its `API_KEY` and the matching
   `KANI_SOLVER_SECRET` on the Kani service (both commented out in the compose file) — and
   re-publish its port, since nothing outside the compose network can reach it otherwise.

**Keep this solver private.** `/v1` executes caller-supplied JavaScript against named browser
profiles, so an exposed instance is remote code execution against your host. `API_KEY` gates only
`/v1` — `/` and `/health` stay open for capability checks and container healthchecks, which carry
no secrets.

## Capacity and diagnostics

| Variable | Purpose |
|---|---|
| `KANI_DB_READ_POOL_SIZE` | SQLite read-pool size |
| `KANI_SLOW_QUERY_THRESHOLD_MS` | Warn about slower SQL statements |
| `KANI_LOG_FORMAT` | `text` or `json` logging |
| `KANI_LOG_BUFFER_SIZE` | In-memory log-viewer capacity |
| `KANI_JOB_SHUTDOWN_TIMEOUT_SECONDS` | Graceful job-drain period |
| `KANI_WASM_MODULE_CACHE_DIR` | Persistent compiled-WASM cache |
| `KANI_IMAGE_PROXY_MAX_MEMORY_MB` | Image-proxy memory ceiling |
| `RUST_LOG` | `tracing` filter |

Settings exist for scan/download concurrency, retention, disk warnings, and thumbnail formats.
Use the UI unless an environment-seeded deployment is specifically required.

## Validate a deployment

After changing boot configuration, restart Kani and check the startup log, `/ready`,
**Settings → Diagnostics**, and the system capabilities response. A variable accepted by the
process is not proof that the feature's external dependency is installed or reachable.
