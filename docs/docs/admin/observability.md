# Observability and Diagnostics

## Logs

Kani uses `tracing`. Set `RUST_LOG` to a filter and `KANI_LOG_FORMAT=json` for structured output:

```bash
RUST_LOG=kani=debug,tower_http=warn
KANI_LOG_FORMAT=json
```

Container logs are available with `docker compose logs -f kani`. Authorised administrators can
also open **Admin → Logs**. The in-memory viewer has a bounded buffer and is not a durable log
archive.

Use debug or trace logging only while investigating a problem. Logs can contain source URLs,
titles, usernames, and operational metadata even though secret values are filtered from support
bundles.

## Liveness and readiness

Kani exposes `/health` and `/healthz` for liveness, plus `/ready` and `/readyz` for readiness.
Liveness answers whether the process is running. Readiness additionally indicates whether it can
serve normal work. Load balancers should use readiness; process supervisors can use liveness.

## Prometheus metrics

`GET /metrics` requires a general API token scoped to `metrics:read`:

```bash
curl -H "Authorization: Bearer <token>" https://kani.example.com/metrics
```

Create the token under **Settings → Clients** with only the metrics scope. Metrics can expose
extension names, upstream hosts, route classes, error counts, and runtime behavior, so the endpoint
does not accept anonymous scrapes even though it bypasses the normal cookie middleware.

## Diagnostics

**Settings → Diagnostics** aggregates system, database, storage, job, bandwidth, browser,
degradation, circuit-breaker, and recent-error state. Refresh the page after reproducing a problem
and correlate timestamps with logs and jobs.

Source-specific state is under **Settings → Source health**. A circuit breaker can explain why a
source is not being called even when its WASM module loads successfully.

## Support bundles

The diagnostics surface can produce support information with sensitive configuration filtered.
Review any bundle before sharing it: library names, paths, hostnames, logs, and extension metadata
may still be private even when cryptographic secrets are omitted.

Kani does not currently export OpenTelemetry traces. Use structured logs, Prometheus metrics,
diagnostics, and job history for supported observability.
