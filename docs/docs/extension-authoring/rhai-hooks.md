# Rhai Scripting

Rhai extends YAML sources in two constrained ways:

- Pure functions transform DSL values.
- Request hooks mutate a request or react to the final HTTP status.

Scripts are inline YAML values. The runtime does not load arbitrary `.rhai` files from a sibling
directory.

## Pure functions

Declare named expression bodies under `scripts.pure`:

```yaml
scripts:
  pure:
    slugify: |
      let value = arg0.to_lower();
      value.replace(" ", "-")

endpoints:
  search:
    fields:
      slug: 'self.first("h2").text().user.slugify()'
```

The DSL receiver is `arg0`; explicit arguments become `arg1`, `arg2`, and so on. Pure functions
accept and return strings, integers, numbers, booleans, string lists, or null. They cannot access
HTTP, preferences, filters, or cache directly.

## Request hooks

`pre_request` runs before a request is sent. `on_status` selects a script by exact status, class
such as `5xx`, or `default` after the HTTP client's own retry policy has finished.

Hooks may be top-level or inside an endpoint. Source-level `pre_request` runs before the endpoint
hook. Endpoint hooks then refine behavior for that operation.

```yaml
metadata:
  rate_limit:
    max_hook_requests: 2

pre_request: |
  let token = ctx.cache.get("auth", "token");
  if token != "" {
    req.set_header("Authorization", "Bearer " + token);
  }
  proceed()

on_status:
  "401": |
    let token = ctx.cache.get("auth", "pending_token");
    ctx.cache.put("auth", "token", token, 3600);
    retry()
  "5xx": |
    fail("upstream unavailable")

endpoints:
  popular:
    pre_request: |
      req.set_header("X-Endpoint", "popular");
      proceed()
```

## Bindings

| Binding | Availability | Operations |
|---|---|---|
| `req` | Both hook types | Read URL/method and set URL, header, query, or body values |
| `ctx` | Both hook types | Read filters and preferences; access declared cache namespaces |
| `response` | `on_status` only | Read integer status and response headers |

Cache operations are `ctx.cache.get(namespace, key)` and
`ctx.cache.put(namespace, key, value, ttl_seconds)`. A missing cache entry returns an empty string.

## Required return actions

Every hook returns one action:

| Action | Effect |
|---|---|
| `proceed()` | Continue with the current request or response |
| `retry()` | Send the request again |
| `retry_after(seconds)` | Retry after a bounded delay |
| `fail(reason)` | Stop with a network extension error |

Hook retries count against `metadata.rate_limit.max_hook_requests`. The smart HTTP client already
retries selected rate-limit and gateway failures before `on_status` runs. Do not use a `5xx` hook
to create an unbounded second retry policy.

## Sandbox limits

The default operation, string, and array ceilings are controlled by `KANI_RHAI_MAX_OPS`,
`KANI_RHAI_MAX_STRING`, and `KANI_RHAI_MAX_ARRAY`. Expression depth and call levels are also
bounded. `eval`, module import/export, closures, and function pointers are disabled.

These limits apply to untrusted extension logic. If a script exceeds one, simplify the script
before raising a server-wide ceiling.

## Validate and test

`kani-cli validate` parses every pure function and hook in a sandbox, validates status selectors,
and checks that hook expressions return supported forms. Use REPL record/replay for repeatable
upstream fixtures instead of testing only against a live changing site.

```bash
cargo run -p kani-cli -- validate my-source.yaml
cargo run -p kani-cli -- repl inspect my-source.yaml
```
