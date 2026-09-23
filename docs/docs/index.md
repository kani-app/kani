# Kani

Kani is a self-hosted manga and comics server. It organises a multi-user library, downloads
chapters through sandboxed source extensions, and provides a responsive web reader and OPDS
catalogue.

![Kani library showing fictional manga with plain-colour covers](img/screenshots/library-desktop.png)

## Highlights

- **Extension system** — install signed WASM or declarative YAML sources, or write your own.
- **Library automation** — scan followed titles and apply download rules for language, scanlator,
  volume, and other conditions.
- **CBZ and ComicInfo** — import existing archives and preserve portable metadata.
- **Reader and offline access** — track progress by page, cache chapters in the browser, or connect
  an OPDS-capable reader.
- **Tracker sync** — link reading progress to AniList and MyAnimeList.
- **Multi-user security** — inherited roles, granular permissions, scoped API tokens, session
  management, and optional two-factor authentication.
- **Operations** — scheduled backups, background-job tracking, diagnostics, Prometheus metrics,
  email, and signed webhooks.

## Start here

- [Quickstart](getting-started/quickstart.md) — build and run Kani with Docker Compose.
- [Library guide](user/library.md) — browse, organise, and search your collection.
- [Settings guide](user/settings.md) — find account and server controls.
- [Administration](admin/configuration.md) — configure and operate an instance.
- [Extension authoring](extension-authoring/yaml-schema.md) — create a source extension.
- [Architecture](developer/architecture.md) — understand the host, services, and WASM boundary.

## License

Kani is released under the MIT licence. See the
[Disclaimer](https://github.com/kani-app/kani/blob/main/DISCLAIMER.md) for details.
