//! Prometheus metric registration, runtime gauge synchronization, and authenticated scraping.

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use axum::response::IntoResponse;
use axum::routing::get;
use axum_prometheus::metrics_exporter_prometheus::PrometheusHandle;
use axum_prometheus::{EndpointLabel, PrometheusMetricLayer, PrometheusMetricLayerBuilder};
use std::sync::OnceLock;

/// Bounds metric cardinality by grouping unmatched static assets and SPA fallbacks.
/// Matched routes carry a `MatchedPath` and do not pass through this function.
fn collapse_unmatched_path(path: &str) -> String {
    for prefix in ["/js/", "/css/", "/fonts/", "/icons/", "/locales/"] {
        if path.starts_with(prefix) {
            return format!("{prefix}*");
        }
    }
    // Everything else is the SPA fallback or a 404 — one server-side operation,
    // and an open door for cardinality if a crawler walks random URLs.
    "/other".to_string()
}

pub fn prometheus() -> &'static (PrometheusMetricLayer<'static>, PrometheusHandle) {
    static PROM: OnceLock<(PrometheusMetricLayer<'static>, PrometheusHandle)> = OnceLock::new();
    PROM.get_or_init(|| {
        PrometheusMetricLayerBuilder::new()
            .with_endpoint_label_type(EndpointLabel::MatchedPathWithFallbackFn(
                collapse_unmatched_path,
            ))
            .with_default_metrics()
            .build_pair()
    })
}

pub fn describe() {
    let _ = prometheus();

    metrics::describe_counter!(
        "kani_log_errors_total",
        "Total ERROR-level log events emitted"
    );
    metrics::describe_gauge!("kani_sse_clients", "Currently connected SSE clients");
    metrics::describe_gauge!("kani_jobs_running", "Background jobs currently running");
    metrics::describe_counter!(
        "kani_jobs_failed_total",
        "Background jobs that terminated in failure"
    );
    metrics::describe_gauge!(
        "kani_downloads_active",
        "Chapter downloads currently in flight"
    );
    metrics::describe_counter!(
        "kani_wasm_calls_total",
        "Extension backend calls, by extension and method"
    );
    metrics::describe_histogram!(
        "kani_wasm_call_duration_seconds",
        "Extension backend call latency, by extension and method"
    );
    metrics::describe_counter!(
        "kani_wasm_call_errors_total",
        "Extension backend calls that returned an error"
    );
    metrics::describe_gauge!(
        "kani_circuit_open",
        "1 when the per-host circuit breaker is open, 0 otherwise"
    );
    metrics::describe_counter!("kani_v8_calls_total", "Total calls into the V8 subprocess");
    metrics::describe_counter!(
        "kani_v8_process_restarts_total",
        "Times the V8 subprocess has been restarted"
    );
    metrics::describe_counter!(
        "kani_v8_graceful_shutdowns_total",
        "Managed V8 or Chromium processes stopped through a graceful shutdown"
    );
    metrics::describe_counter!(
        "kani_v8_forced_terminations_total",
        "Managed V8 or Chromium processes that required forced process-tree termination"
    );
    metrics::describe_counter!(
        "kani_browser_reuses_total",
        "Browser-backed calls that reused an existing Chromium instance"
    );
    metrics::describe_counter!(
        "kani_browser_recovery_launches_total",
        "Chromium launches that required an isolated recovery profile"
    );
    metrics::describe_counter!(
        "kani_browser_challenges_total",
        "Cloudflare challenges detected by browser payload capture"
    );
    metrics::describe_counter!(
        "kani_browser_page_close_timeouts_total",
        "Browser pages that exceeded the bounded close timeout"
    );
    metrics::describe_counter!(
        "kani_browser_solver_attempts_total",
        "FlareSolverr credential requests made for browser challenges"
    );
    metrics::describe_counter!(
        "kani_browser_solver_successes_total",
        "Successful FlareSolverr credential requests for browser challenges"
    );
    metrics::describe_counter!(
        "kani_browser_solver_failures_total",
        "Failed FlareSolverr credential requests for browser challenges"
    );

    metrics::counter!("kani_log_errors_total").increment(0);
    metrics::gauge!("kani_sse_clients").set(0.0);
    metrics::gauge!("kani_jobs_running").set(0.0);
    metrics::gauge!("kani_downloads_active").set(0.0);
    sync_runtime_counters();
}

pub fn router(state: crate::state::AppState) -> Router {
    Router::new()
        .route("/metrics", get(render))
        .with_state(MetricsState {
            handle: prometheus().1.clone(),
            app: state,
        })
}

#[derive(Clone)]
/// Router state pairing the Prometheus renderer with application authorization state.
pub(crate) struct MetricsState {
    handle: PrometheusHandle,
    app: crate::state::AppState,
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| !v.is_empty())
}

/// Scraping requires an API token scoped to `metrics:read` — the sole
/// credential. Unlike /health, this endpoint discloses extension names, upstream
/// host names via the circuit gauge, version and error counts, so no
/// credential means no access.
///
/// A scoped token is revocable per scraper and audited via `last_used_at`,
/// neither of which a shared env-var secret offered.
async fn authorized(app: &crate::state::AppState, headers: &HeaderMap) -> bool {
    let Some(raw) = bearer(headers) else {
        return false;
    };
    match app.service.authenticate_api_token(raw).await {
        Ok(Some(auth)) => {
            auth.kind == kani_app::service::api_tokens::TokenKind::Api
                && auth
                    .scopes
                    .contains(&kani_app::permissions::Permission::Metrics(
                        kani_app::permissions::Metrics::Read,
                    ))
        }
        _ => false,
    }
}

pub(crate) fn sync_runtime_counters() {
    let stats = kani_core::v8_process::browser_stats();
    metrics::counter!("kani_v8_calls_total").absolute(stats.calls_total);
    metrics::counter!("kani_v8_process_restarts_total").absolute(stats.restarts);
    metrics::counter!("kani_v8_graceful_shutdowns_total").absolute(stats.graceful_shutdowns);
    metrics::counter!("kani_v8_forced_terminations_total").absolute(stats.forced_terminations);
    metrics::counter!("kani_browser_solver_attempts_total").absolute(stats.solver_attempts);
    metrics::counter!("kani_browser_solver_successes_total").absolute(stats.solver_successes);
    metrics::counter!("kani_browser_solver_failures_total").absolute(stats.solver_failures);
}

async fn render(State(state): State<MetricsState>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&state.app, &headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            "metrics require an API token scoped to `metrics:read` \
             (Settings -> Clients), sent as `Authorization: Bearer <token>`",
        )
            .into_response();
    }
    sync_runtime_counters();
    (StatusCode::OK, state.handle.render()).into_response()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn unmatched_static_paths_collapse_to_one_label_each() {
        assert_eq!(
            collapse_unmatched_path("/js/dist/chunk-GAAGDHTX.js"),
            "/js/*"
        );
        assert_eq!(
            collapse_unmatched_path("/js/dist/settings-4Y7Q3MMA.js"),
            "/js/*"
        );
        assert_eq!(collapse_unmatched_path("/css/main.css"), "/css/*");
        assert_eq!(collapse_unmatched_path("/fonts/x.woff2"), "/fonts/*");
    }

    #[test]
    fn arbitrary_paths_share_a_single_bucket() {
        assert_eq!(collapse_unmatched_path("/library"), "/other");
        assert_eq!(collapse_unmatched_path("/settings"), "/other");
        assert_eq!(collapse_unmatched_path("/wp-admin.php"), "/other");
    }

    fn auth_header(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, value.parse().unwrap());
        h
    }

    #[test]
    fn no_credential_is_not_a_credential() {
        assert!(
            bearer(&HeaderMap::new()).is_none(),
            "no header means no scraping; /metrics discloses extension and \
             upstream host names"
        );
        assert!(
            bearer(&auth_header("Bearer ")).is_none(),
            "an empty bearer must not be treated as a token"
        );
    }

    #[test]
    fn only_a_bearer_is_read_as_a_token() {
        assert_eq!(bearer(&auth_header("Bearer kani_abc")), Some("kani_abc"));
        assert!(bearer(&auth_header("Basic kani_abc")).is_none());
        assert!(
            bearer(&auth_header("kani_abc")).is_none(),
            "a bare value is not a bearer credential"
        );
    }
}
