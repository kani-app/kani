# Request Flow

## Browser REST request

```text
browser
  -> reverse proxy and TLS
  -> Axum middleware
       request/body budgets
       tracing and security headers
       rate limiting
       session or bearer-token authentication
       CSRF for state-changing cookie requests
  -> route permission guard
  -> kani-web handler
  -> AppService method
  -> SQLite / source registry / job manager / integration
  -> response and optional SSE invalidation
```

Static assets, health/readiness, metrics, OPDS, image proxy, chapter files, exports, and `/rest`
have different top-level routing. A path exempt from the normal auth guard may still authenticate
inside its own handler; metrics is the standard example.

## Cookie and token authorization

A browser login creates a server-side session and HTTP-only cookie. Read-only traffic establishes
the readable `kani_csrf` value; later state-changing requests echo it in `X-CSRF-Token`. The route
guard loads effective inherited permissions for the session user.

A bearer request hashes and looks up the API token, rejects expiry or revocation, loads the owner's
current permissions, and intersects those with declared token scopes. Bearer requests skip CSRF.

## Source call

```text
REST/service operation
  -> SourceRegistry lease for source ID
  -> active interpreted-YAML or WASM backend
  -> provider operation
       -> SmartClient request policy and retry
       -> optional pre_request / on_status hooks
       -> HTML or JSON parse handle
       -> blueprint evaluator
       -> typed provider result
  -> service persistence or response DTO
```

The registry lease prevents a hot update from invalidating an in-flight backend. Installation and
update validate the replacement before swapping it into the registry.

## Declarative extraction call

A blueprint can include its request. The host sends the request, parses the response, evaluates
bindings and scalars, iterates the container, evaluates fields, performs bounded chaining where
declared, and returns a JSON handle containing rows and scalars. The guest decodes only the final
result.

## Background job

```text
handler / recurring scheduler
  -> JobManager submission and deduplication
  -> persisted queued state
  -> bounded worker execution
  -> progress + SSE events
  -> completed / failed / paused / cancelled state
  -> owning UI refreshes durable REST state
```

SSE is a notification channel, not the durable record. Reconnecting clients refetch the resource
or job rather than reconstructing state from events they may have missed.

## Signal-to-pixel rule

When adding a value, trace every hop: compute, persist, select, serialize, cache, and render. A
source probe without a caller, a selected column omitted from a response projection, or an SSE
atom with no subscriber is an incomplete feature even if its local tests pass.
