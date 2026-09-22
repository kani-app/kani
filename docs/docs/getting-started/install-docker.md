# Install with Docker

## Choose a deployment source

The default Compose file pulls the published image:

```bash
git clone https://github.com/kani-app/kani.git
cd kani
docker compose pull
docker compose up -d
```

The release pipeline builds a signed, multi-arch (`amd64`/`arm64`) image and publishes it to
`ghcr.io/kani-app/kani`, tagged with its version and, only for an actual release (never a
prerelease), `latest`. Verify a pulled image's signature before trusting it in production:

```bash
cosign verify ghcr.io/kani-app/kani:latest \
  --certificate-identity-regexp "^https://github.com/kani-app/kani/" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com"
```

To build from source instead — required before `v1.0.0` exists and `latest` first resolves, or
to run an unreleased change — replace the `image:` line in `docker-compose.yml` with the
commented-out `build:` block right above it, then:

```bash
git checkout <release-tag>
docker compose up --build -d
```

## Compose configuration

The essential shape of the included Compose service is:

```yaml
services:
  kani:
    image: ghcr.io/kani-app/kani:latest
    ports:
      - "8242:8242"
    volumes:
      - ./data:/data
      - ./library:/library
    environment:
      KANI_BIND: "0.0.0.0:8242"
      KANI_SECURE_COOKIES: "false"
    restart: unless-stopped
```

Follow the logs after starting it:

```bash
docker compose logs -f kani
```

## Volumes and ownership

| Mount | Contents |
|---|---|
| `/data` | SQLite database, installed extensions, browser profiles, and generated key files |
| `/library` | Covers, downloaded chapter data, and archives |

Both mounts must be writable by UID and GID 1000, which the image handles for you automatically:
the container starts as root, chowns `/data` and `/library`, then drops to the unprivileged `kani`
user before running Kani itself — no action needed even on a bind mount Docker just created. This
is not a LinuxServer.io-style image, though: there is no `PUID`/`PGID` remapping, and setting those
variables has no effect. If you bind-mount a `/library` that already holds files owned by another
user (migrating an existing collection, for example), chown it yourself first — the automatic
chown only touches the mount roots, not recursively, so startup stays fast with a large library.

`/data/secret.key` protects stored credentials, and `/data/proxy.key` signs image-proxy URLs.
Losing either key can make the corresponding stored data unusable, so back them up with `kani.db`.

Do not put the SQLite database on NFS, SMB, or another filesystem that does not provide SQLite's
required locking semantics. The library may live on a separate large disk.

## Ports and binding

Kani listens on TCP port 8242 by default. `KANI_BIND` controls both the address and port:

```yaml
environment:
  KANI_BIND: "127.0.0.1:8242"
```

Binding to loopback is appropriate for a host-networked reverse proxy. Inside a normal Compose
network, leave Kani listening on `0.0.0.0` and publish the host port only where required.

## Common environment variables

| Variable | Default | Purpose |
|---|---|---|
| `KANI_BIND` | `0.0.0.0:8242` | Listen address and port |
| `KANI_DATA_DIR` | process working directory | Database and generated-key directory |
| `KANI_LIBRARY_DIR` | setting value | Override the library directory at startup |
| `KANI_STATIC_DIR` | `static` | Frontend asset directory for nonstandard binary layouts |
| `KANI_SECURE_COOKIES` | `false` | Mark cookies Secure; enable behind HTTPS |
| `KANI_CORS_ORIGIN` | request origin | Restrict browser API access to one origin |
| `KANI_PUBLIC_INSTANCE` | `false` | Enable the internet-facing hardened profile |
| `KANI_ALLOW_REMOTE_SETUP` | `false` | Permit first-account setup outside private networks |
| `KANI_ALLOW_REGISTRATION` | setting value | Startup override for self-registration |
| `KANI_SECRET_KEY` | generated file | Inline 64-character hex credential-encryption key |
| `KANI_SECRET_KEY_FILE` | — | Read the credential key from a mounted secret |
| `KANI_PROXY_SECRET` | generated file | Base64url 32-byte image-proxy signing key |
| `KANI_LOG_FORMAT` | `text` | `text` or structured `json` output |
| `RUST_LOG` | `error` | `tracing` filter |

See [Configuration](../admin/configuration.md) for rate limits, browser support, extension
installation policy, database pools, and which values should instead be changed in the UI.

## Optional image features

The Dockerfile supports a build argument for a larger optional dependency:

```yaml
build:
  args:
    INSTALL_KCC: "true"
```

`INSTALL_KCC` adds Kindle Comic Converter for MOBI/AZW3 export. It increases image size and should
be enabled only when needed.

Sources that capture browser payloads need no build argument: that work runs in the solver, which
is a separate container. See [Configuration](../admin/configuration.md) for the solver image and
its key.

## Health and restart behavior

The image health check calls `GET /health`. Kani also exposes `/healthz`, `/ready`, and `/readyz`;
readiness can fail while the process is still alive.

Exit code 0 is a clean shutdown. Exit code 42 requests a supervised restart, which
`restart: unless-stopped` handles.

## Upgrade

Take a backup, review the intended release's notes, then pull and restart:

```bash
docker compose pull
docker compose up -d
```

Building from source instead: fetch the tag and rebuild —

```bash
git fetch --tags
git checkout <release-tag>
docker compose up --build -d
```

Database migrations run during startup. Do not assume that a migrated database can be opened by an
older binary; follow the release's migration and rollback policy. See [Upgrades](../admin/upgrade.md).
