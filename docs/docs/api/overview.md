# API Overview

Kani's application API is mounted under `/rest`. OPDS, image and chapter delivery, health,
readiness, metrics, and the frontend have separate top-level routes.

## Source of truth

Route request and response schemas are generated from the Rust handlers with `utoipa`. CI checks
that the REST router and OpenAPI document cover the same surface. Use that document for the exact
endpoint inventory instead of copying a hand-written list from this guide.

Debug builds serve Swagger UI and the document at `/api-docs`; release builds, including normal
production images, do not mount them. To inspect the current checkout:

```bash
cargo run -p kani-web
```

Then open `http://localhost:8242/api-docs`. A document generated from a different commit may not
describe the server you operate.

## Authentication modes

- The web application uses an HTTP-only session cookie and CSRF protection.
- General API tokens use `Authorization: Bearer <token>` and explicit permission scopes.
- OPDS reader tokens are accepted only by the OPDS surface.
- OPDS can also support client-specific HTTP authentication behavior documented by its generated
  route contract.

See [Authentication](auth.md). A path bypassing global cookie middleware is not necessarily
anonymous: metrics and OPDS perform their own authentication.

## Content types and errors

Most REST requests and responses use JSON. Upload, backup, archive, image, SSE, and export routes
use other media types described in OpenAPI.

Errors use an appropriate HTTP status and a JSON body, commonly with `error` and sometimes
`message` or structured details. Clients must branch on status and machine-readable fields rather
than matching an English message.

Common classes are:

| Status | Meaning |
|---|---|
| `400` / `422` | Invalid request or validation failure |
| `401` | Missing or invalid authentication |
| `403` | Authentication succeeded but permission, CSRF, or policy denied the action |
| `404` | Resource not found or not visible |
| `409` | Current state conflicts with the requested change |
| `429` | Rate or login-attempt limit; inspect `Retry-After` where supplied |

## Pagination

Pagination is not globally uniform. Some endpoints use query parameters, some source routes carry
page and page-size in the path, and some lists use a simple limit. Responses may expose
`items`, `has_next_page`, `total_pages`, `total`, or domain-specific field names. Follow the schema
for the endpoint being called.

## Live updates

The web client consumes Server-Sent Events for download progress, jobs, scans, invalidation, and
other live state. Reconnecting clients must tolerate missed transient events and refresh the
corresponding REST resource rather than treating SSE as a durable event log.

## Compatibility

API stability and migration guarantees are defined by the release's published stability policy.
Do not infer compatibility from an unreleased branch or from a route that happens to appear in one
OpenAPI document.
