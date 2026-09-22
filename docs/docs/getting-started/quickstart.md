# Quickstart

This guide builds and runs Kani from the checked-out repository with Docker Compose. Published
image coordinates and tags are release-specific; see [Install with Docker](install-docker.md)
before replacing the build with an image reference.

## Prerequisites

- Git
- Docker Engine and Docker Compose v2
- At least 1 GB of free memory

## 1. Get Kani

```bash
git clone https://github.com/kani-app/kani.git
cd kani
```

For a long-lived installation, check out a release tag rather than an arbitrary development
commit. The included Compose file persists application data in `./data` and downloaded files in
`./library`.

## 2. Start the server

```bash
docker compose up --build -d
docker compose logs -f kani
```

When the health check passes, open [http://localhost:8242](http://localhost:8242).

## 3. Create the administrator

A new database has no accounts. The setup page asks you to choose the first administrator's
username, email address, and password, then signs you in.

Setup closes after the first account is created. It is accepted only from a loopback
or private address unless `KANI_ALLOW_REMOTE_SETUP=true` is set. Prefer a LAN connection or SSH
tunnel. If the override is unavoidable, remove it immediately after creating the account and
restart Kani.

## 4. Complete onboarding

Choose the library directory and, if a repository is configured, install a first source. In the
standard container the library directory is `/library`, mapped to `./library` on the host.

## 5. Add a source and follow a title

Open **Sources**. Add a signed extension repository or install an extension using a method allowed
by the operator, then select the source in the sidebar. Search for a title, open it, and add it to
the library. Kani can scan followed titles for new chapters and apply the title's download rules.

See [Sources](../admin/sources.md) for installation methods and the repository trust model.

## Next steps

- [Library guide](../user/library.md)
- [Reverse proxy](reverse-proxy.md)
- [Backup and restore](../admin/backup-restore.md)
- [Users and roles](../admin/users-roles.md)
