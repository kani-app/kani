# Webhooks

Kani sends signed JSON requests for selected application events. Webhook delivery is tracked as
background work and has a delivery history.

## Create an endpoint

Open **Settings → Webhooks**, add an HTTPS URL, select events, and set a strong secret. A webhook
can be enabled or disabled without deleting its configuration. Per-manga overrides can narrow
which configured hooks apply to a title.

Kani protects outbound requests against unsafe destinations. Private, loopback, and otherwise
blocked targets are rejected rather than treated as an internal integration channel.

## Event types

| Event | Data |
|---|---|
| `chapter.new` | Manga identity plus newly discovered chapter IDs and names |
| `manga.added` | Manga identity and source ID |
| `manga.deleted` | Manga identity |
| `chapter.downloaded` | Chapter and manga identity plus names |
| `scan.completed` | Counts of scanned and failed titles |

## Envelope and signature

```json
{
  "event": "chapter.downloaded",
  "timestamp": "<RFC3339 timestamp>",
  "data": {
    "chapter_id": 42,
    "manga_id": 7,
    "manga_name": "Example Manga",
    "chapter_name": "Chapter 1"
  }
}
```

When a secret is configured, Kani sends:

```http
X-Kani-Signature: sha256=<hex-digest>
```

The digest is HMAC-SHA256 of the exact raw body. Verify it before decoding or acting on the JSON,
use constant-time comparison, and reject a missing or malformed signature.

## Delivery operations

The delivery log records event type, payload, HTTP status or error, and timestamp. Retry behavior
is handled by the job framework. Make receivers idempotent because a timeout can occur after the
receiver processed a request but before Kani observed the response.

Use the in-app test or a deliberately triggered low-impact event to verify routing, TLS, signature
handling, and response time. See [Webhooks API](../api/webhooks.md) for complete field examples.
