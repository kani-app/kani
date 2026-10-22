# Extension Repositories

An extension repository is a static signed catalogue. It lets Kani discover extensions, verify
their artifacts, install them, and relate installed sources to later updates.

## Trust model

The repository maintainer signs `index.json` with Ed25519. Each artifact is separately signed by
its author, and its SHA-256 digest is recorded in the index.

```text
maintainer key -> index.json
                   |
                   +-> author key + digest -> extension artifact
```

On first use, Kani displays the maintainer fingerprint and requires explicit trust-on-first-use
confirmation. Verify the fingerprint through another channel controlled by the maintainer. Kani
pins it and rejects a later index signed by a different key until an administrator deliberately
reviews the change.

The transport must use HTTPS, but TLS alone does not replace signature verification.

## Add and trust a repository

Open **Sources**, choose repository management, and enter the base URL. Review the fetched name and
fingerprint, compare it with the maintainer's published value, then confirm trust. Adding a URL
without verifying the fingerprint gives TOFU no useful security property.

An operator can bootstrap a selected official repository with `KANI_OFFICIAL_REPO_URL` and its
Ed25519 key. This is deployment trust, not an assertion that any arbitrary URL is official.

## Browse, install, and update

Refresh a repository to fetch a newly signed index. Browse its extensions and review version,
description, language, content rating, network access, minimum Kani version, and required
capabilities before installing.

Kani verifies the index signature, artifact URL policy, digest, author signature, compatibility,
and capabilities before replacing files or database state. An update failure leaves the existing
source available.

Removing a repository removes its catalogue and trust record but does not silently uninstall
sources already obtained from it. Those sources lose repository-managed update discovery until an
appropriate repository is trusted again.

## Key changes and compromise

A pinned maintainer-key change is a security event, not a routine refresh error. Confirm a planned
rotation through a trusted announcement before accepting it. If compromise is suspected, block
the URL, disable affected sources, preserve logs and artifacts, and wait for a verified recovery
statement.

Author-key rotation affects the artifacts signed by that author. Maintainer-key rotation affects
the entire index trust anchor. They require different recovery steps.

## Blocking and installation policy

Administrators can block repository URLs before the TOFU flow. For a locked-down deployment, set
`KANI_SOURCE_INSTALL_ALLOWED=false`; existing sources continue to run, while upload, URL, and
repository installations are rejected.

Repository and artifact fetches use Kani's SSRF protections. Private and loopback destinations are
not a supported way to host a repository.

## Backup implications

Logical backups include repository trust and block records when that component is selected.
Disaster recovery must also preserve installed artifacts and the database. Re-adding a repository
without its old trust record starts a new TOFU decision and must be verified again.

See [Publishing and distribution](../extension-authoring/publishing.md) to create a repository.
