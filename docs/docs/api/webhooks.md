# Webhooks API

See [Webhooks](../admin/webhooks.md) for configuration and operations.

## Envelope

Every delivery uses this shape:

```json
{
  "event": "<event type>",
  "timestamp": "<RFC3339 timestamp>",
  "data": {}
}
```

The enum variant is serialized inside `data` using snake-case field names. Receivers should ignore
unknown extra fields for forward compatibility and route on the top-level event name.

## Event schemas

### `chapter.new`

```json
{
  "manga_id": 7,
  "manga_name": "Example Manga",
  "chapter_count": 2,
  "chapter_ids": [41, 42],
  "chapter_names": ["Chapter 1", "Chapter 2"]
}
```

### `manga.added`

```json
{
  "manga_id": 7,
  "manga_name": "Example Manga",
  "source_id": 3
}
```

### `manga.deleted`

```json
{
  "manga_id": 7,
  "manga_name": "Example Manga"
}
```

### `chapter.downloaded`

```json
{
  "chapter_id": 42,
  "manga_id": 7,
  "manga_name": "Example Manga",
  "chapter_name": "Chapter 1"
}
```

### `scan.completed`

```json
{
  "total_scanned": 12,
  "failed_count": 1
}
```

## Verify the signature

With a configured secret, the request includes:

```http
X-Kani-Signature: sha256=<hex-digest>
```

The digest is HMAC-SHA256 over the exact raw body. Verify before JSON parsing:

```python
import hashlib
import hmac

def verify(secret: str, body: bytes, header: str) -> bool:
    expected = "sha256=" + hmac.new(
        secret.encode(), body, hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(expected, header)
```

Reject a request when the secret is expected and the header is absent, malformed, or mismatched.
Store an idempotency record based on stable payload fields if processing the same event twice would
be harmful; retries can follow ambiguous network failures.
