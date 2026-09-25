//! Upstream HTTP client policy: SSRF-safe redirects, retries, caching, budgets, throttling, and
//! source circuit breaking.

use crate::{error::Result, network::ValidatingResolver};
use arc_swap::ArcSwap;
use futures::{TryStream, TryStreamExt};
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use serde_json::json;
use std::{collections::HashMap, num::NonZeroU32, sync::Arc};

const MAX_RETRIES: u32 = 3;
const BASE_DELAY: std::time::Duration = std::time::Duration::from_secs(5);
/// Ceiling on an HTML body buffered for challenge detection. Real catalogue
/// pages are a few hundred kilobytes; anything past this is not a page we were
/// going to parse anyway.
const MAX_HTML_BODY_BYTES: usize = 16 * 1024 * 1024;
/// Ceiling on a response body handed to a guest extension.
const MAX_HTTP_RESPONSE_BYTES: usize = 15 * 1024 * 1024;
/// Ceiling on an operator-configured option-set fetch. An option set is
/// kilobytes; anything past this is not a document we were going to parse.
const MAX_OPTION_SET_BYTES: usize = 4 * 1024 * 1024;

const CREDENTIAL_TTL_SECS: u64 = 3600;
const RETRY_AFTER_CAP_SECS: u64 = 60;
const CIRCUIT_OPEN_THRESHOLD: u32 = 5;
const CIRCUIT_COOLDOWN_SECS: u64 = 30;
/// FlareSolverr's own `maxTimeout` is 60 s; the HTTP client wrapping it needs a
/// slightly larger ceiling so a legitimately slow solve completes but an
/// unreachable solver cannot hang the request indefinitely.
const SOLVER_TIMEOUT_SECS: u64 = 65;
const REQUEST_TIMEOUT_SECS: u64 = 35;
/// `kani.capture` spends its budget on the challenge solve, the reload, and
/// only then the capture, so the solver's overall ceiling has to exceed the
/// caller's capture timeout or the solve alone can exhaust it.
const SOLVER_CAPTURE_SOLVE_HEADROOM_MS: u64 = 60_000;
const SOLVER_CAPTURE_TRANSPORT_BUFFER_MS: u64 = 5_000;
const SOLVER_SESSION_TTL_MINUTES: u64 = 5;

/// How long the solver itself keeps a session before expiring it. The host reaps
/// against this rather than an operator setting, so the two cannot disagree.
pub const fn solver_session_ttl() -> std::time::Duration {
    std::time::Duration::from_secs(SOLVER_SESSION_TTL_MINUTES * 60)
}
const SOLVER_SESSION_CONTROL_TIMEOUT_SECS: u64 = 10;
/// Allows the complete retry schedule and one solver attempt to finish.
const WHOLE_CALL_DEADLINE_SECS: u64 = 120;

/// Timing knobs for the retry/circuit/solver machinery. Production uses
/// [`Timings::default`]; tests override them via [`SmartClient::with_timings`]
/// to drive the same real code paths without minute-long backoff waits.
#[derive(Clone, Copy)]
pub struct Timings {
    pub retry_base_delay: std::time::Duration,
    pub retry_jitter: std::time::Duration,
    pub circuit_cooldown: std::time::Duration,
    pub solver_timeout: std::time::Duration,
    pub credential_ttl: std::time::Duration,
    /// Per-attempt ceiling applied in addition to the client's transport timeout.
    pub request_timeout: std::time::Duration,
    /// Wall-clock ceiling across all attempts, backoffs, and challenge solving.
    pub whole_call_deadline: std::time::Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            retry_base_delay: BASE_DELAY,
            retry_jitter: std::time::Duration::from_millis(1000),
            circuit_cooldown: std::time::Duration::from_secs(CIRCUIT_COOLDOWN_SECS),
            solver_timeout: std::time::Duration::from_secs(SOLVER_TIMEOUT_SECS),
            credential_ttl: std::time::Duration::from_secs(CREDENTIAL_TTL_SECS),
            request_timeout: std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS),
            whole_call_deadline: std::time::Duration::from_secs(WHOLE_CALL_DEADLINE_SECS),
        }
    }
}

/// Ceilings on how many bytes a single response may occupy in memory.
///
/// Production uses [`Budgets::default`]; a test overrides them via
/// [`SmartClient::with_budgets`] so an oversized-body path can be driven with a
/// few kilobytes instead of allocating the real megabyte ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    pub max_html_body_bytes: usize,
    pub max_http_response_bytes: usize,
    pub max_option_set_bytes: usize,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            max_html_body_bytes: MAX_HTML_BODY_BYTES,
            max_http_response_bytes: MAX_HTTP_RESPONSE_BYTES,
            max_option_set_bytes: MAX_OPTION_SET_BYTES,
        }
    }
}

pub struct RateState {
    limiter: DefaultDirectRateLimiter,
    semaphore: Arc<tokio::sync::Semaphore>,
}

fn host_of(url: &str) -> Option<String> {
    url.parse::<url::Url>()
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
}

/// Everything the caller supplied except what authenticates them.
fn without_credential_headers(h: &rquest::header::HeaderMap) -> rquest::header::HeaderMap {
    let mut out = h.clone();
    out.remove(rquest::header::AUTHORIZATION);
    out.remove(rquest::header::COOKIE);
    out.remove(rquest::header::PROXY_AUTHORIZATION);
    out
}

pub struct HostCircuit {
    consecutive_failures: std::sync::atomic::AtomicU32,
    open_until: std::sync::Mutex<Option<std::time::Instant>>,
}

impl HostCircuit {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            consecutive_failures: std::sync::atomic::AtomicU32::new(0),
            open_until: std::sync::Mutex::new(None),
        })
    }
}

/// Only the transient gateway faults 502 and 504 are retried in-request — the
/// upstream may recover within a couple of backoff windows.
///
/// 429 is deliberately NOT here: a rate-limit is not a transient blip. Sleeping
/// on `Retry-After` inside the request (capped, up to MAX_RETRIES) pins a worker
/// and a pooled connection for as long as a minute, and a large `Retry-After`
/// overruns the caller's outer timeout — turning a clean 429 into a misleading
/// timeout. Instead the 429 is surfaced immediately; the evaluator marks it with
/// the HTTP-status sentinel so extraction reports a typed RateLimited (carrying
/// `Retry-After`), and the download job's own retry policy reschedules with that
/// backoff. 503 is excluded too — it is the Cloudflare challenge signal.
fn is_retryable(status: rquest::StatusCode) -> bool {
    matches!(
        status,
        rquest::StatusCode::BAD_GATEWAY | rquest::StatusCode::GATEWAY_TIMEOUT
    )
}

/// Parses integer or HTTP-date `Retry-After`, applying the configured cap and
/// exponential fallback.
fn compute_delay(
    headers: Option<&rquest::header::HeaderMap>,
    attempt: u32,
    base_delay: std::time::Duration,
) -> std::time::Duration {
    let backoff = base_delay * 2u32.pow(attempt);
    headers
        .and_then(|h| h.get(rquest::header::RETRY_AFTER))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.parse::<u64>()
                .ok()
                .map(|secs| secs.min(RETRY_AFTER_CAP_SECS))
                .or_else(|| {
                    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc2822)
                        .ok()
                        .map(|dt| {
                            let now = time::OffsetDateTime::now_utc();
                            (dt - now).whole_seconds().max(0) as u64
                        })
                        .map(|secs| secs.min(RETRY_AFTER_CAP_SECS))
                })
        })
        .map(std::time::Duration::from_secs)
        .unwrap_or(backoff)
}

fn jitter(max: std::time::Duration) -> std::time::Duration {
    use rand::RngExt;
    let max_ms = max.as_millis() as u64;
    if max_ms == 0 {
        return std::time::Duration::ZERO;
    }
    std::time::Duration::from_millis(rand::rng().random_range(0u64..max_ms))
}

#[derive(Clone, Default)]
pub struct CachedCredentials {
    cookies: String,
    user_agent: Option<String>,
    stored_at: Option<std::time::Instant>,
    challenge_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BrowserChallengeCredentials {
    pub cookie_header: String,
    pub user_agent: String,
    pub from_cache: bool,
}

/// What the configured solver can do for us, established by probing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverCapability {
    /// No solver URL is set at all, as distinct from one that does not answer.
    NotConfigured,
    Unreachable,
    Unauthorized,
    /// Cannot run scripted capture (stock FlareSolverr, Byparr), but is still
    /// fully usable for ordinary HTTP challenge solving.
    Basic,
    Capture,
}

impl SolverCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::Unreachable => "unreachable",
            Self::Unauthorized => "unauthorized",
            Self::Basic => "basic",
            Self::Capture => "capture",
        }
    }
}

/// Suffixes that name a machine on the local network rather than a routable
/// host, so plain HTTP to them is not a warning-worthy exposure.
const LOCAL_SUFFIXES: &[&str] = &[".local", ".lan", ".home", ".internal", ".localdomain"];

/// True when solver traffic would cross a routable network in the clear.
///
/// Deliberately narrow: warning on every non-HTTPS solver would fire on
/// `http://flaresolverr:8191`, which is the documented compose setup, and a
/// warning that fires on the recommended configuration teaches people to
/// ignore warnings.
pub fn solver_transport_is_exposed(url: &str) -> bool {
    let Ok(parsed) = url.parse::<url::Url>() else {
        return false;
    };
    if parsed.scheme() != "http" {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();

    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                !(v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified())
            }
            std::net::IpAddr::V6(v6) => {
                let unique_local = v6.octets()[0] & 0xfe == 0xfc;
                let link_local = v6.segments()[0] & 0xffc0 == 0xfe80;
                !(v6.is_loopback() || v6.is_unspecified() || unique_local || link_local)
            }
        };
    }

    if host == "localhost" || LOCAL_SUFFIXES.iter().any(|s| host.ends_with(s)) {
        return false;
    }

    host.contains('.')
}

/// Absent means the solver is unauthenticated; a key is never invented.
fn solver_secret() -> Option<&'static str> {
    static SECRET: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    SECRET
        .get_or_init(|| {
            std::env::var("KANI_SOLVER_SECRET")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .as_deref()
}

/// Failure classes callers use to distinguish an incompatible stock solver from a failed capture.
#[derive(Debug)]
pub enum SolverCaptureError {
    Unsupported,
    Unauthorized,
    Unreachable,
    /// The solver ran the script but it never called `passPayload`. Distinct
    /// from `Failed` because the solver itself is healthy: a caller retrying a
    /// harvest should treat this as "try again", not "the solver is broken".
    ScriptProducedNothing(String),
    Failed(String),
}

impl std::fmt::Display for SolverCaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(
                f,
                "the configured solver does not support the 'kani.capture' command; \
                 a Kani-compatible FlareSolverr image is required for browser sources \
                 behind a managed challenge"
            ),
            Self::Unauthorized => write!(
                f,
                "the solver rejected Kani's key; check KANI_SOLVER_SECRET matches \
                 the solver's API_KEY"
            ),
            Self::Unreachable => write!(f, "no solver is reachable at the configured URL"),
            Self::ScriptProducedNothing(message) => write!(f, "{message}"),
            Self::Failed(message) => write!(f, "{message}"),
        }
    }
}

#[derive(Clone)]
pub struct CircuitOpenedEvent {
    pub host: String,
    pub failure_count: u32,
}

struct CachedHeaders {
    etag: Option<String>,
    last_modified: Option<String>,
}

pub struct ConditionalGetCache(dashmap::DashMap<String, CachedHeaders>);

impl Default for ConditionalGetCache {
    fn default() -> Self {
        Self(dashmap::DashMap::new())
    }
}

impl ConditionalGetCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn apply_to(&self, url: &str, mut builder: rquest::RequestBuilder) -> rquest::RequestBuilder {
        if let Some(cached) = self.0.get(url) {
            if let Some(ref etag) = cached.etag {
                if let Ok(v) = rquest::header::HeaderValue::from_str(etag) {
                    builder = builder.header(rquest::header::IF_NONE_MATCH, v);
                }
            } else if let Some(ref lm) = cached.last_modified
                && let Ok(v) = rquest::header::HeaderValue::from_str(lm)
            {
                builder = builder.header(rquest::header::IF_MODIFIED_SINCE, v);
            }
        }
        builder
    }

    fn record(&self, url: &str, headers: &rquest::header::HeaderMap) {
        let etag = headers
            .get(rquest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let last_modified = headers
            .get(rquest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if etag.is_some() || last_modified.is_some() {
            self.0.insert(
                url.to_string(),
                CachedHeaders {
                    etag,
                    last_modified,
                },
            );
        }
    }
}

pub enum SmartResponse {
    Normal(rquest::Response),
    Buffered {
        status: rquest::StatusCode,
        url: rquest::Uri,
        headers: rquest::header::HeaderMap,
        body: bytes::Bytes,
    },
    NotModified {
        url: rquest::Uri,
        headers: rquest::header::HeaderMap,
    },
}

async fn response_chunk(response: &mut rquest::Response) -> Result<Option<bytes::Bytes>> {
    use http_body_util::BodyExt;
    while let Some(frame) = response.frame().await {
        if let Ok(data) = frame?.into_data() {
            return Ok(Some(data));
        }
    }
    Ok(None)
}

impl SmartResponse {
    pub fn status(&self) -> rquest::StatusCode {
        match self {
            SmartResponse::Normal(r) => r.status(),
            SmartResponse::Buffered { status, .. } => *status,
            SmartResponse::NotModified { .. } => rquest::StatusCode::NOT_MODIFIED,
        }
    }

    pub fn url(&self) -> &rquest::Uri {
        match self {
            SmartResponse::Normal(r) => r.uri(),
            SmartResponse::Buffered { url, .. } => url,
            SmartResponse::NotModified { url, .. } => url,
        }
    }

    pub fn headers(&self) -> &rquest::header::HeaderMap {
        match self {
            SmartResponse::Normal(r) => r.headers(),
            SmartResponse::Buffered { headers, .. } => headers,
            SmartResponse::NotModified { headers, .. } => headers,
        }
    }

    pub async fn bytes(self) -> Result<bytes::Bytes> {
        match self {
            SmartResponse::Normal(r) => Ok(r.bytes().await?),
            SmartResponse::Buffered { body, .. } => Ok(body),
            SmartResponse::NotModified { .. } => Ok(bytes::Bytes::new()),
        }
    }

    pub async fn text(self) -> Result<String> {
        match self {
            SmartResponse::Normal(r) => Ok(r.text().await?),
            SmartResponse::Buffered { body, .. } => Ok(String::from_utf8_lossy(&body).to_string()),
            SmartResponse::NotModified { .. } => Ok(String::new()),
        }
    }

    pub async fn chunk(&mut self) -> Result<Option<bytes::Bytes>> {
        match self {
            SmartResponse::Normal(r) => response_chunk(r).await,
            SmartResponse::Buffered { body, .. } => {
                if body.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(std::mem::take(body)))
                }
            }
            SmartResponse::NotModified { .. } => Ok(None),
        }
    }

    pub async fn bytes_limited(self, max_bytes: usize) -> Result<bytes::Bytes> {
        let content_length = self
            .headers()
            .get(rquest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok());

        let stream = Box::pin(futures::stream::unfold(self, |mut resp| async move {
            match resp.chunk().await {
                Ok(Some(bytes)) => Some((Ok(bytes), resp)),
                Ok(None) => None,
                Err(e) => Some((Err(e), resp)),
            }
        }));

        collect_bytes_limited(stream, content_length, max_bytes).await
    }

    /// Reads at most `max_bytes` and stops, discarding the rest — a *prefix*,
    /// not a whole body that happens to be small.
    ///
    /// Unlike `bytes_limited`, this accepts a response larger than the requested
    /// prefix because servers are permitted to ignore a `Range` request.
    pub async fn bytes_prefix(mut self, max_bytes: usize) -> Result<bytes::Bytes> {
        let mut buf = bytes::BytesMut::new();
        while buf.len() < max_bytes {
            match self.chunk().await? {
                Some(chunk) => {
                    let take = (max_bytes - buf.len()).min(chunk.len());
                    buf.extend_from_slice(&chunk[..take]);
                }
                None => break,
            }
        }
        Ok(buf.freeze())
    }
}

const REDIRECT_LIMIT: usize = 10;

/// The one SSRF egress decision, applied to the first request and to every redirect
/// hop: refuse a forbidden IP literal, the hole the DNS-only resolver never sees.
/// A host the source has been granted is exempt; `allow_loopback` (tests only) exempts
/// loopback literals and nothing else.
fn egress_forbidden(
    allow_loopback: &std::sync::atomic::AtomicBool,
    grants: &crate::network::LocalGrants,
    url: &str,
) -> bool {
    crate::network::is_forbidden_url_host(url)
        && !grants.permits_url(url)
        && !(allow_loopback.load(std::sync::atomic::Ordering::Relaxed)
            && crate::network::is_loopback_url_host(url))
}

/// Redirect policy for the auto-following client: follow up to `REDIRECT_LIMIT`
/// hops, refusing a forbidden egress target per hop and, when given, any hop the
/// source's host policy would not allow as a first request.
fn ssrf_aware_redirect_policy(
    allow_loopback: Arc<std::sync::atomic::AtomicBool>,
    grants: Arc<crate::network::LocalGrants>,
    allowed_host: Option<crate::wasm::AllowedHost>,
) -> rquest::redirect::Policy {
    rquest::redirect::Policy::custom(move |attempt| {
        if attempt.previous.len() >= REDIRECT_LIMIT {
            return attempt.error(Box::<dyn std::error::Error + Send + Sync>::from(
                "too many redirects",
            ));
        }
        if egress_forbidden(&allow_loopback, &grants, &attempt.uri.to_string()) {
            return attempt.error(Box::<dyn std::error::Error + Send + Sync>::from(
                "redirect to a forbidden host refused",
            ));
        }
        if let Some(policy) = &allowed_host
            && let Err(reason) = policy.allows_host(attempt.uri.host().unwrap_or_default())
        {
            return attempt.error(Box::<dyn std::error::Error + Send + Sync>::from(format!(
                "redirect refused: {reason}"
            )));
        }
        attempt.follow()
    })
}

impl std::fmt::Debug for SmartClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartClient").finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct SmartClient {
    client: rquest::Client,
    pub credentials: Arc<ArcSwap<HashMap<String, CachedCredentials>>>,
    solver_url: Arc<ArcSwap<Option<String>>>,
    solver_sessions: Arc<dashmap::DashMap<String, std::time::Instant>>,
    solver_capture_support: Arc<std::sync::atomic::AtomicU8>,
    solver_client: Arc<std::sync::OnceLock<rquest::Client>>,
    pub solving: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub host_circuits: Arc<dashmap::DashMap<String, Arc<HostCircuit>>>,
    pub rate_states: Arc<dashmap::DashMap<String, Arc<RateState>>>,
    pub circuit_event_tx: tokio::sync::broadcast::Sender<CircuitOpenedEvent>,
    pub cond_cache: Arc<ConditionalGetCache>,
    timings: Timings,
    budgets: Budgets,
    /// When false (production), any request or redirect to a forbidden IP literal
    /// (private/loopback/metadata) is refused. Tests set it true to reach a local
    /// server on loopback; other forbidden ranges stay refused. Shared with the
    /// redirect policy closure so it is read live.
    allow_loopback_egress: Arc<std::sync::atomic::AtomicBool>,
    /// Private hosts this client may reach on behalf of one source ([`Self::with_local_grants`]).
    local_grants: Arc<crate::network::LocalGrants>,
    /// Whether the inner client follows redirects itself, so a granted copy keeps the same shape.
    follows_redirects: bool,
}

impl SmartClient {
    /// Overrides the retry/circuit/solver timings (test seam — see [`Timings`]).
    pub fn with_timings(mut self, timings: Timings) -> Self {
        self.timings = timings;
        self
    }

    /// Overrides the response-size ceilings (test seam — see [`Budgets`]).
    pub fn with_budgets(mut self, budgets: Budgets) -> Self {
        self.budgets = budgets;
        self
    }

    /// The response-size ceilings this client enforces.
    pub fn budgets(&self) -> Budgets {
        self.budgets
    }

    /// Allow egress to loopback IP literals (test seam so a `TestOrigin` on
    /// `127.0.0.1` is reachable). Other forbidden ranges stay refused.
    ///
    /// Gated to test builds: it exists only under `cfg(test)` or the `test-util`
    /// feature, neither of which a release binary compiles (dev-deps are excluded
    /// from a production build). So there is **no way to disable the SSRF guard in
    /// production** — the field is constructed `false` and has no public mutator.
    #[cfg(any(test, feature = "test-util"))]
    pub fn with_allow_loopback_egress(self, allow: bool) -> Self {
        self.allow_loopback_egress
            .store(allow, std::sync::atomic::Ordering::Relaxed);
        self
    }

    /// Builds the underlying client. `follows_redirects` selects the auto-following,
    /// SSRF-checked policy (source extraction) or none (`safe_get` follows by hand).
    fn build_inner(
        follows_redirects: bool,
        allow_loopback: &Arc<std::sync::atomic::AtomicBool>,
        grants: &Arc<crate::network::LocalGrants>,
    ) -> Result<rquest::Client> {
        let resolver = ValidatingResolver::new()?.with_grants(Arc::clone(grants));
        let redirect = if follows_redirects {
            ssrf_aware_redirect_policy(Arc::clone(allow_loopback), Arc::clone(grants), None)
        } else {
            rquest::redirect::Policy::none()
        };
        Ok(rquest::Client::builder()
            .emulation(rquest_util::Emulation::Chrome130)
            .redirect(redirect)
            .dns_resolver(Arc::new(resolver))
            .pool_idle_timeout(std::time::Duration::from_secs(300))
            .pool_max_idle_per_host(100)
            .timeout(std::time::Duration::from_secs(35))
            .build()?)
    }

    /// A copy of this client that may also reach the private hosts one source has been
    /// granted. Rate limits, circuits and stored credentials stay shared with the original.
    pub fn with_local_grants(&self, grants: crate::network::LocalGrants) -> Result<Self> {
        let grants = Arc::new(grants);
        let client =
            Self::build_inner(self.follows_redirects, &self.allow_loopback_egress, &grants)?;
        Ok(Self {
            client,
            local_grants: grants,
            ..self.clone()
        })
    }

    pub fn new(solver_url: Option<String>) -> Result<Self> {
        let allow_loopback_egress = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let local_grants = Arc::new(crate::network::LocalGrants::default());
        let client = Self::build_inner(true, &allow_loopback_egress, &local_grants)?;

        let (circuit_event_tx, _) = tokio::sync::broadcast::channel(32);
        Ok(Self {
            client,
            credentials: Arc::new(ArcSwap::from_pointee(HashMap::new())),
            solver_url: Arc::new(ArcSwap::from_pointee(solver_url)),
            solver_sessions: Arc::new(dashmap::DashMap::new()),
            solver_capture_support: Arc::new(std::sync::atomic::AtomicU8::new(0)),
            solver_client: Arc::new(std::sync::OnceLock::new()),
            solving: Arc::new(dashmap::DashMap::new()),
            host_circuits: Arc::new(dashmap::DashMap::new()),
            rate_states: Arc::new(dashmap::DashMap::new()),
            circuit_event_tx,
            cond_cache: Arc::new(ConditionalGetCache::new()),
            timings: Timings::default(),
            budgets: Budgets::default(),
            allow_loopback_egress,
            local_grants,
            follows_redirects: true,
        })
    }

    pub fn new_proxy(
        solver_url: Option<String>,
        credentials: Arc<ArcSwap<HashMap<String, CachedCredentials>>>,
        solving: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
        host_circuits: Arc<dashmap::DashMap<String, Arc<HostCircuit>>>,
        circuit_event_tx: tokio::sync::broadcast::Sender<CircuitOpenedEvent>,
    ) -> Result<Self> {
        let allow_loopback_egress = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let local_grants = Arc::new(crate::network::LocalGrants::default());
        let client = Self::build_inner(false, &allow_loopback_egress, &local_grants)?;

        Ok(Self {
            client,
            credentials,
            solver_url: Arc::new(ArcSwap::from_pointee(solver_url)),
            solver_sessions: Arc::new(dashmap::DashMap::new()),
            solver_capture_support: Arc::new(std::sync::atomic::AtomicU8::new(0)),
            solver_client: Arc::new(std::sync::OnceLock::new()),
            solving,
            host_circuits,
            rate_states: Arc::new(dashmap::DashMap::new()),
            circuit_event_tx,
            cond_cache: Arc::new(ConditionalGetCache::new()),
            timings: Timings::default(),
            budgets: Budgets::default(),
            allow_loopback_egress,
            local_grants,
            follows_redirects: false,
        })
    }

    /// Registers a rate limit under the same base-domain key used by request lookup.
    pub fn register_rate_limit(&self, domain: &str, cfg: &kani_shared::extension::RateLimitConfig) {
        let domain = &base_domain(domain);
        let period_ns = (1_000_000_000.0 / cfg.requests_per_second.max(0.001)) as u64;
        let burst = NonZeroU32::new(cfg.burst.max(1)).unwrap_or(NonZeroU32::MIN);
        let quota = Quota::with_period(std::time::Duration::from_nanos(period_ns))
            .expect("rate limit period must be > 0")
            .allow_burst(burst);
        let limiter = RateLimiter::direct(quota);
        let max_concurrent = cfg.max_concurrent.max(1) as usize;
        let state = Arc::new(RateState {
            limiter,
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrent)),
        });
        self.rate_states.insert(domain.clone(), state);
    }

    /// Removes the rate limit state for a domain. Called when a source is removed or reloaded.
    pub fn deregister_rate_limit(&self, domain: &str) {
        self.rate_states.remove(&base_domain(domain));
    }

    fn circuit_for(&self, domain: &str) -> Arc<HostCircuit> {
        self.host_circuits
            .entry(domain.to_string())
            .or_insert_with(HostCircuit::new)
            .clone()
    }

    fn is_circuit_open(&self, domain: &str) -> bool {
        let Some(circuit) = self.host_circuits.get(domain) else {
            return false;
        };
        let guard = circuit.open_until.lock().expect("circuit mutex poisoned");
        guard
            .map(|until| std::time::Instant::now() < until)
            .unwrap_or(false)
    }

    fn record_success(&self, domain: &str) {
        let Some(circuit) = self.host_circuits.get(domain) else {
            return;
        };
        circuit
            .consecutive_failures
            .store(0, std::sync::atomic::Ordering::Relaxed);
        *circuit.open_until.lock().expect("circuit mutex poisoned") = None;
    }

    fn record_failure(&self, domain: &str) {
        let circuit = self.circuit_for(domain);
        let prev = circuit
            .consecutive_failures
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if prev + 1 >= CIRCUIT_OPEN_THRESHOLD {
            let now = std::time::Instant::now();
            let until = now + self.timings.circuit_cooldown;
            let mut guard = circuit.open_until.lock().expect("circuit mutex poisoned");
            let was_open = guard.map(|t| now < t).unwrap_or(false);
            *guard = Some(until);
            drop(guard);
            tracing::warn!(
                "Circuit opened for {} after {} consecutive failures (cooldown {}s)",
                domain,
                prev + 1,
                CIRCUIT_COOLDOWN_SECS,
            );
            if !was_open {
                let _ = self.circuit_event_tx.send(CircuitOpenedEvent {
                    host: domain.to_string(),
                    failure_count: prev + 1,
                });
            }
        }
    }

    pub(crate) async fn send_request(&self, request: rquest::Request) -> Result<SmartResponse> {
        let mut request = request;
        self.refuse_forbidden_egress(&request.uri().to_string())?;

        let domain = request.uri().host().map(base_domain).unwrap_or_default();
        let creds_map = self.credentials.load();
        if let Some(creds) = creds_map.get(&domain) {
            let expired = creds
                .stored_at
                .map(|t| t.elapsed() > self.timings.credential_ttl)
                .unwrap_or(true);

            if expired {
                tracing::debug!("Credentials for {} have expired, dropping", domain);
                drop(creds_map);
                self.credentials.rcu(|old| {
                    let mut m = (**old).clone();
                    m.remove(&domain);
                    Arc::new(m)
                });
            } else {
                if let Ok(val) = rquest::header::HeaderValue::from_str(&creds.cookies) {
                    request.headers_mut().insert(rquest::header::COOKIE, val);
                } else {
                    tracing::warn!(
                        "Stored cookies for {} contained invalid header characters, skipping",
                        domain
                    );
                }

                if let Some(ref ua) = creds.user_agent
                    && let Ok(val) = rquest::header::HeaderValue::from_str(ua)
                {
                    request
                        .headers_mut()
                        .insert(rquest::header::USER_AGENT, val);
                }
            }
        }

        if self.is_circuit_open(&domain) {
            return Err(crate::error::Error::Other(format!(
                "Circuit open for {domain}: host temporarily unavailable"
            )));
        }

        let rate_state = self.rate_states.get(&domain).map(|r| Arc::clone(&*r));
        if let Some(ref state) = rate_state {
            state.limiter.until_ready().await;
        }
        let _semaphore_permit = if let Some(ref state) = rate_state {
            state.semaphore.clone().acquire_owned().await.ok()
        } else {
            None
        };

        let mut current_request = request;
        let mut attempt = 0;
        let call_deadline = tokio::time::Instant::now() + self.timings.whole_call_deadline;

        loop {
            if tokio::time::Instant::now() >= call_deadline {
                return Err(crate::error::Error::Other(
                    "HTTP request exceeded its overall deadline".into(),
                ));
            }
            let request_clone_for_retry = current_request.try_clone();

            let executed = tokio::time::timeout(
                self.timings.request_timeout,
                self.client.execute(current_request),
            )
            .await;
            let resp = match executed {
                Ok(Ok(r)) => r,
                // Redirect-policy rejections are deterministic for this chain and must
                // not consume the retry schedule.
                Ok(Err(e)) if e.is_redirect() => return Err(e.into()),
                // A transport error or an elapsed per-attempt timeout: retry if we can.
                Ok(Err(_)) | Err(_)
                    if attempt < MAX_RETRIES && request_clone_for_retry.is_some() =>
                {
                    let delay = compute_delay(None, attempt, self.timings.retry_base_delay)
                        + jitter(self.timings.retry_jitter);
                    tracing::warn!(
                        "HTTP request failed/timed out, retrying in {:?} (attempt {}/{})",
                        delay,
                        attempt + 1,
                        MAX_RETRIES,
                    );
                    self.record_failure(&domain);
                    tokio::time::sleep(delay).await;
                    current_request = request_clone_for_retry
                        .expect("guarded by is_some() in the match arm above");
                    attempt += 1;
                    continue;
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => {
                    return Err(crate::error::Error::Other(
                        "HTTP request exceeded the per-attempt timeout".into(),
                    ));
                }
            };
            let status = resp.status();

            if is_retryable(status) {
                if attempt < MAX_RETRIES
                    && let Some(next_req) = request_clone_for_retry
                {
                    let delay =
                        compute_delay(Some(resp.headers()), attempt, self.timings.retry_base_delay)
                            + jitter(self.timings.retry_jitter);
                    tracing::warn!(
                        "Upstream returned {}, retrying in {:?} (attempt {}/{})",
                        status.as_u16(),
                        delay,
                        attempt + 1,
                        MAX_RETRIES,
                    );
                    tokio::time::sleep(delay).await;
                    current_request = next_req;
                    attempt += 1;
                    continue;
                }
                self.record_failure(&domain);
                return Ok(SmartResponse::Normal(resp));
            }

            if status.is_success() {
                let is_html = resp
                    .headers()
                    .get(rquest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(|ct| ct.contains("text/html"))
                    .unwrap_or(false);

                if is_html {
                    let status = resp.status();
                    let url = resp.uri().clone();
                    let headers = resp.headers().clone();
                    let bytes = collect_bytes_limited(
                        Box::pin(futures::stream::unfold(resp, |mut r| async move {
                            match response_chunk(&mut r).await {
                                Ok(Some(b)) => Some((Ok(b), r)),
                                Ok(None) => None,
                                Err(e) => Some((Err(e), r)),
                            }
                        })),
                        None,
                        self.budgets.max_html_body_bytes,
                    )
                    .await?;
                    let body_str = String::from_utf8_lossy(&bytes);
                    let body_lower = body_str.to_lowercase();

                    let is_challenge = body_lower.contains("just a moment...")
                        || body_lower.contains("enable javascript");

                    if is_challenge
                        && self.solver_url.load().is_some()
                        && request_clone_for_retry.is_some()
                    {
                        let url_str = url.to_string();
                        let resp = self.get_rendered_page_once(&url_str).await?;
                        self.record_success(&domain);
                        return Ok(resp);
                    } else {
                        self.record_success(&domain);
                        return Ok(SmartResponse::Buffered {
                            status,
                            url,
                            headers,
                            body: bytes,
                        });
                    }
                } else {
                    self.record_success(&domain);
                    return Ok(SmartResponse::Normal(resp));
                }
            }

            if (status == rquest::StatusCode::FORBIDDEN
                || status == rquest::StatusCode::SERVICE_UNAVAILABLE)
                && self.solver_url.load().is_some()
                && request_clone_for_retry.is_some()
            {
                let url = resp.uri().to_string();
                let cf_domain = url
                    .parse::<url::Url>()
                    .ok()
                    .and_then(|u| u.host_str().map(base_domain));

                if let Some(ref d) = cf_domain {
                    if self.credentials.load().contains_key(d) {
                        tracing::info!(
                            "Stored credentials for {} returned 403, clearing and re-solving",
                            d
                        );
                    }
                    self.credentials.rcu(|old| {
                        let mut m = (**old).clone();
                        m.remove(d);
                        Arc::new(m)
                    });
                }

                let req_headers = request_clone_for_retry.as_ref().map(|r| r.headers());
                let (new_cookies, new_ua) = self.solve_challenge_once(&url, req_headers).await?;

                if let Some(mut request) = request_clone_for_retry {
                    if let Ok(val) = rquest::header::HeaderValue::from_str(&new_cookies) {
                        request.headers_mut().insert(rquest::header::COOKIE, val);
                    }

                    if let Ok(val) = rquest::header::HeaderValue::from_str(&new_ua) {
                        request
                            .headers_mut()
                            .insert(rquest::header::USER_AGENT, val);
                    }

                    let resp = self.client.execute(request).await?;
                    self.record_success(&domain);
                    return Ok(SmartResponse::Normal(resp));
                }
            }

            return Ok(SmartResponse::Normal(resp));
        }
    }

    /// The redirect policy for one extension's request: the client's own rules plus that
    /// source's host policy, applied to every hop.
    pub fn source_redirect_policy(
        &self,
        allowed_host: crate::wasm::AllowedHost,
    ) -> rquest::redirect::Policy {
        ssrf_aware_redirect_policy(
            Arc::clone(&self.allow_loopback_egress),
            Arc::clone(&self.local_grants),
            Some(allowed_host),
        )
    }

    /// Refuses a first hop to a forbidden IP literal, which the validating resolver never sees.
    fn refuse_forbidden_egress(&self, url: &str) -> Result<()> {
        if egress_forbidden(&self.allow_loopback_egress, &self.local_grants, url) {
            return Err(crate::error::Error::Other(format!(
                "request to a forbidden host refused: {url}"
            )));
        }
        Ok(())
    }

    pub async fn get(&self, url: &str) -> Result<SmartResponse> {
        let request = self.client.get(url).build()?;
        self.send_request(request).await
    }

    /// [`Self::get`] with a source's host policy applied to every redirect hop.
    pub async fn get_for_source(
        &self,
        url: &str,
        allowed_host: crate::wasm::AllowedHost,
    ) -> Result<SmartResponse> {
        let request = self
            .client
            .get(url)
            .redirect(self.source_redirect_policy(allowed_host))
            .build()?;
        self.send_request(request).await
    }

    pub async fn safe_get(
        &self,
        initial_url: &str,
        headers: Option<rquest::header::HeaderMap>,
    ) -> Result<SmartResponse> {
        self.safe_get_impl(initial_url, headers, None).await
    }

    pub async fn safe_get_conditional(
        &self,
        initial_url: &str,
        headers: Option<rquest::header::HeaderMap>,
    ) -> Result<SmartResponse> {
        self.safe_get_impl(initial_url, headers, Some(&self.cond_cache.clone()))
            .await
    }

    async fn safe_get_impl(
        &self,
        initial_url: &str,
        headers: Option<rquest::header::HeaderMap>,
        cond_cache: Option<&ConditionalGetCache>,
    ) -> Result<SmartResponse> {
        // Unified with the auto-following client's policy limit.
        const MAX_REDIRECTS: usize = REDIRECT_LIMIT;

        self.refuse_forbidden_egress(initial_url)?;
        let mut current_url = initial_url.to_string();
        let mut solver_headers = rquest::header::HeaderMap::new();
        let mut solved = false;

        let circuit_domain = initial_url
            .parse::<url::Url>()
            .ok()
            .and_then(|u| u.host_str().map(base_domain))
            .unwrap_or_default();

        if let Ok(parsed) = initial_url.parse::<url::Url>()
            && let Some(domain) = parsed.host_str().map(base_domain)
        {
            let creds_map = self.credentials.load();
            if let Some(creds) = creds_map.get(&domain) {
                let expired = creds
                    .stored_at
                    .map(|t| t.elapsed() > self.timings.credential_ttl)
                    .unwrap_or(true);
                if expired {
                    tracing::debug!("Credentials for {} have expired, clearing", domain);
                    drop(creds_map);
                    self.credentials.rcu(|old| {
                        let mut m = (**old).clone();
                        m.remove(&domain);
                        Arc::new(m)
                    });
                } else {
                    if let Ok(val) = rquest::header::HeaderValue::from_str(&creds.cookies) {
                        solver_headers.insert(rquest::header::COOKIE, val);
                    }
                    if let Some(ref ua) = creds.user_agent
                        && let Ok(val) = rquest::header::HeaderValue::from_str(ua)
                    {
                        solver_headers.insert(rquest::header::USER_AGENT, val);
                    }
                }
            }
        }

        if self.is_circuit_open(&circuit_domain) {
            return Err(crate::error::Error::Other(format!(
                "Circuit open for {circuit_domain}: host temporarily unavailable"
            )));
        }

        let rate_state = self
            .rate_states
            .get(&circuit_domain)
            .map(|r| Arc::clone(&*r));
        if let Some(ref state) = rate_state {
            state.limiter.until_ready().await;
        }
        let _semaphore_permit = if let Some(ref state) = rate_state {
            state.semaphore.clone().acquire_owned().await.ok()
        } else {
            None
        };

        let initial_host = host_of(initial_url);

        let mut redirect_count = 0usize;
        let mut retry_count = 0u32;
        let call_deadline = tokio::time::Instant::now() + self.timings.whole_call_deadline;

        loop {
            if tokio::time::Instant::now() >= call_deadline {
                return Err(crate::error::Error::Other(
                    "safe_get exceeded its overall deadline".into(),
                ));
            }
            let mut req_builder = self.client.get(&current_url);
            // Conditional-GET validators stay pinned to the URL they were
            // stored for; an ETag means nothing to whatever the redirect lands
            // on.
            if current_url == initial_url
                && let Some(cache) = cond_cache
            {
                req_builder = cache.apply_to(initial_url, req_builder);
            }
            if let Some(ref h) = headers {
                req_builder = req_builder.headers(if host_of(&current_url) == initial_host {
                    h.clone()
                } else {
                    without_credential_headers(h)
                });
            }
            if !solver_headers.is_empty() {
                req_builder = req_builder.headers(solver_headers.clone());
            }
            let req = req_builder.build()?;

            let current_headers = req.headers().clone();

            let executed =
                tokio::time::timeout(self.timings.request_timeout, self.client.execute(req)).await;
            let resp = match executed {
                Ok(Ok(r)) => r,
                // A redirect-policy rejection is terminal (see send_request).
                Ok(Err(e)) if e.is_redirect() => return Err(e.into()),
                Ok(Err(_)) | Err(_) if retry_count < MAX_RETRIES => {
                    let delay = compute_delay(None, retry_count, self.timings.retry_base_delay)
                        + jitter(self.timings.retry_jitter);
                    tracing::warn!(
                        "safe_get network error/timeout, retrying in {:?} (attempt {}/{})",
                        delay,
                        retry_count + 1,
                        MAX_RETRIES,
                    );
                    self.record_failure(&circuit_domain);
                    retry_count += 1;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => {
                    return Err(crate::error::Error::Other(
                        "safe_get exceeded the per-attempt timeout".into(),
                    ));
                }
            };

            if is_retryable(resp.status()) && retry_count < MAX_RETRIES {
                let delay = compute_delay(
                    Some(resp.headers()),
                    retry_count,
                    self.timings.retry_base_delay,
                ) + jitter(self.timings.retry_jitter);
                tracing::warn!(
                    "safe_get got {}, retrying in {:?} (attempt {}/{})",
                    resp.status().as_u16(),
                    delay,
                    retry_count + 1,
                    MAX_RETRIES,
                );
                self.record_failure(&circuit_domain);
                retry_count += 1;
                tokio::time::sleep(delay).await;
                continue;
            }

            if resp.status() == rquest::StatusCode::NOT_MODIFIED && cond_cache.is_some() {
                self.record_success(&circuit_domain);
                let url = resp.uri().clone();
                let response_headers = resp.headers().clone();
                return Ok(SmartResponse::NotModified {
                    url,
                    headers: response_headers,
                });
            }

            if resp.status().is_redirection() {
                let location = resp
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        crate::error::Error::Other("redirect with no Location header".into())
                    })?;

                let next = url::Url::parse(&resp.uri().to_string())
                    .and_then(|url| url.join(location))
                    .map_err(|e| {
                        crate::error::Error::Other(format!(
                            "invalid redirect URL '{}': {}",
                            location, e
                        ))
                    })?;

                match next.scheme() {
                    "http" | "https" => {}
                    s => {
                        return Err(crate::error::Error::Other(format!(
                            "redirect to forbidden scheme: {}",
                            s
                        )));
                    }
                }

                // Re-validate the redirect TARGET for SSRF (same decision the
                // auto-follow policy makes — the resolver never sees an IP literal).
                if egress_forbidden(
                    &self.allow_loopback_egress,
                    &self.local_grants,
                    next.as_str(),
                ) {
                    return Err(crate::error::Error::Other(format!(
                        "redirect to a forbidden host refused: {next}"
                    )));
                }

                redirect_count += 1;
                if redirect_count >= MAX_REDIRECTS {
                    return Err(crate::error::Error::Other(format!(
                        "too many redirects following '{}'",
                        initial_url
                    )));
                }
                current_url = next.to_string();
            } else if (resp.status() == rquest::StatusCode::FORBIDDEN
                || resp.status() == rquest::StatusCode::SERVICE_UNAVAILABLE)
                && !solved
                && self.solver_url.load().is_some()
            {
                let url = resp.uri().to_string();
                let domain = url
                    .parse::<url::Url>()
                    .ok()
                    .and_then(|u| u.host_str().map(base_domain));

                if let Some(domain) = domain {
                    if self.credentials.load().contains_key(&domain) {
                        tracing::info!(
                            "Stored credentials for {} returned 403, clearing and re-solving",
                            domain
                        );
                    }
                    self.credentials.rcu(|old| {
                        let mut m = (**old).clone();
                        m.remove(&domain);
                        Arc::new(m)
                    });
                }

                let (cookies, ua) = self
                    .solve_challenge_once(&url, Some(&current_headers))
                    .await?;

                if let Ok(val) = rquest::header::HeaderValue::from_str(&cookies) {
                    solver_headers.insert(rquest::header::COOKIE, val);
                }
                if let Ok(val) = rquest::header::HeaderValue::from_str(&ua) {
                    solver_headers.insert(rquest::header::USER_AGENT, val);
                }

                solved = true;
            } else {
                self.record_success(&circuit_domain);
                if let Some(cache) = cond_cache {
                    cache.record(initial_url, resp.headers());
                }
                return Ok(SmartResponse::Normal(resp));
            }
        }
    }

    pub fn subscribe_circuit_events(&self) -> tokio::sync::broadcast::Receiver<CircuitOpenedEvent> {
        self.circuit_event_tx.subscribe()
    }

    pub fn list_circuits(&self) -> Vec<serde_json::Value> {
        let now = std::time::Instant::now();
        self.host_circuits
            .iter()
            .map(|entry| {
                let host = entry.key().clone();
                let circuit = entry.value();
                let failures = circuit
                    .consecutive_failures
                    .load(std::sync::atomic::Ordering::Relaxed);
                let guard = circuit.open_until.lock().expect("circuit mutex poisoned");
                let is_open = guard.map(|t| now < t).unwrap_or(false);
                let open_for_secs = guard.and_then(|t| {
                    if now < t {
                        Some(t.saturating_duration_since(now).as_secs())
                    } else {
                        None
                    }
                });
                drop(guard);
                serde_json::json!({
                    "host": host,
                    "is_open": is_open,
                    "consecutive_failures": failures,
                    "open_for_secs": open_for_secs,
                })
            })
            .collect()
    }

    pub fn reset_circuit(&self, host: &str) {
        if let Some(circuit) = self.host_circuits.get(host) {
            circuit
                .consecutive_failures
                .store(0, std::sync::atomic::Ordering::Relaxed);
            *circuit.open_until.lock().expect("circuit mutex poisoned") = None;
        }
    }

    pub fn inner(&self) -> &rquest::Client {
        &self.client
    }

    async fn solve_challenge(
        &self,
        url: &str,
        headers: Option<&rquest::header::HeaderMap>,
    ) -> Result<(String, String)> {
        let guard = self.solver_url.load();
        let solver_url = guard
            .as_deref()
            .ok_or_else(|| crate::error::Error::Other("No solver URL configured".into()))?;

        let client = self.solver_http()?;

        let mut body = json!({
          "cmd": "request.get",
          "url": url,
          "maxTimeout": 60000
        });

        if let Some(h) = headers {
            let mut header_map = serde_json::Map::new();
            for (k, v) in h {
                if let Ok(v_str) = v.to_str() {
                    header_map.insert(k.as_str().to_string(), json!(v_str));
                }
            }
            if !header_map.is_empty() {
                body.as_object_mut()
                    .expect("body was constructed as a json! object literal above")
                    .insert("headers".to_string(), json!(header_map));
            }
        }

        let response = Self::solver_request(client, solver_url)
            .body(body.to_string())
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(crate::error::Error::Other(format!(
                "FlareSolverr returned HTTP {}: check your solver URL is correct (should end in /v1)",
                response.status()
            )));
        }

        let response: serde_json::Value = response.json().await?;

        let status = response["status"].as_str().unwrap_or("");
        if status != "ok" {
            return Err(crate::error::Error::Other(format!(
                "FlareSolverr challenge failed with status '{}': {}",
                status,
                response["message"].as_str().unwrap_or("no message")
            )));
        }

        let ua = response["solution"]["userAgent"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        let cookies = response["solution"]["cookies"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|c| {
                format!(
                    "{}={}",
                    c["name"].as_str().unwrap_or_default(),
                    c["value"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<String>>()
            .join("; ");

        Ok((cookies, ua))
    }

    async fn solve_challenge_once(
        &self,
        url: &str,
        headers: Option<&rquest::header::HeaderMap>,
    ) -> Result<(String, String)> {
        let base = url
            .parse::<url::Url>()
            .ok()
            .and_then(|u| u.host_str().map(base_domain))
            .unwrap_or_else(|| url.to_string());

        let mutex = self
            .solving
            .entry(base.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();

        let _guard = mutex.lock().await;

        let creds = self.credentials.load();
        if let Some(c) = creds.get(&base)
            && !c.cookies.is_empty()
        {
            tracing::debug!("Challenge for {} already solved, reusing cookies", base);
            return Ok((c.cookies.clone(), c.user_agent.clone().unwrap_or_default()));
        }

        tracing::info!("Solving challenge for domain {}", base);
        let (cookies, ua) = self.solve_challenge(url, headers).await?;
        self.store_credentials(url, &cookies, &ua);
        Ok((cookies, ua))
    }

    pub fn solver_is_configured(&self) -> bool {
        self.solver_configured()
    }

    pub(crate) fn solver_configured(&self) -> bool {
        self.solver_url
            .load()
            .as_deref()
            .is_some_and(|url| !url.trim().is_empty())
    }

    /// One pooled client for every solver call. Building one per request threw
    /// away the connection, so each capture paid a fresh handshake.
    fn solver_http(&self) -> Result<&rquest::Client> {
        if let Some(client) = self.solver_client.get() {
            return Ok(client);
        }
        let built = rquest::Client::builder()
            .timeout(self.timings.solver_timeout)
            .build()?;
        let _ = self.solver_client.set(built);
        self.solver_client
            .get()
            .ok_or_else(|| crate::error::Error::Other("solver client unavailable".into()))
    }

    fn solver_request_with(
        client: &rquest::Client,
        solver_url: &str,
        timeout: Option<std::time::Duration>,
    ) -> rquest::RequestBuilder {
        let builder = Self::solver_request(client, solver_url);
        match timeout {
            Some(timeout) => builder.timeout(timeout),
            None => builder,
        }
    }

    fn solver_request(client: &rquest::Client, solver_url: &str) -> rquest::RequestBuilder {
        let builder = client
            .post(solver_url)
            .header("Content-Type", "application/json");
        match solver_secret() {
            Some(secret) => builder.header("X-Api-Key", secret),
            None => builder,
        }
    }

    fn solver_index_url(solver_url: &str) -> Option<String> {
        let mut parsed = solver_url.parse::<url::Url>().ok()?;
        parsed.set_path("/");
        parsed.set_query(None);
        Some(parsed.to_string())
    }

    /// Whether the configured solver keeps its browser off private addresses, as it
    /// advertises with `kani.egress-guard/1` on its index. `None` when no solver is
    /// configured or its index cannot be read, since then nothing is known.
    pub async fn solver_has_egress_guard(&self) -> Option<bool> {
        let guard = self.solver_url.load();
        let url = guard.as_deref().filter(|url| !url.trim().is_empty())?;
        let index_url = Self::solver_index_url(url)?;
        let response = self.solver_http().ok()?.get(&index_url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body = response.json::<serde_json::Value>().await.ok()?;
        Some(
            body["capabilities"]
                .as_array()
                .is_some_and(|caps| caps.iter().any(|c| c == "kani.egress-guard/1")),
        )
    }

    /// Establishes what the configured solver can do, and caches it. The index
    /// is unauthenticated while the commands are not, so both are checked.
    pub(crate) async fn probe_solver_capability(&self, url: &str) -> SolverCapability {
        let Ok(client) = self.solver_http() else {
            return SolverCapability::Unreachable;
        };

        let Some(index_url) = Self::solver_index_url(url) else {
            return SolverCapability::Unreachable;
        };
        let Ok(index) = client.get(&index_url).send().await else {
            return SolverCapability::Unreachable;
        };
        if !index.status().is_success() {
            return SolverCapability::Unreachable;
        }
        let Ok(body) = index.json::<serde_json::Value>().await else {
            return SolverCapability::Unreachable;
        };

        let capabilities: Vec<&str> = body["capabilities"]
            .as_array()
            .map(|values| values.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let sessions = capabilities.contains(&"kani.capture/2");
        let capture = sessions || capabilities.contains(&"kani.capture/1");

        let probe = Self::solver_request(client, url)
            .body(json!({ "cmd": "sessions.list" }).to_string())
            .send()
            .await;
        match probe {
            Ok(response) if response.status().as_u16() == 401 => SolverCapability::Unauthorized,
            Ok(_) => {
                let state = if !capture {
                    2
                } else if sessions {
                    3
                } else {
                    1
                };
                self.solver_capture_support
                    .store(state, std::sync::atomic::Ordering::Relaxed);
                if capture {
                    SolverCapability::Capture
                } else {
                    SolverCapability::Basic
                }
            }
            Err(_) => SolverCapability::Unreachable,
        }
    }

    /// What the last probe established, without issuing one. Diagnostics is a
    /// read-only surface and should not make a network call to render.
    pub fn cached_solver_capability(&self) -> Option<SolverCapability> {
        if !self.solver_configured() {
            return Some(SolverCapability::NotConfigured);
        }
        match self
            .solver_capture_support
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            1 | 3 => Some(SolverCapability::Capture),
            2 => Some(SolverCapability::Basic),
            _ => None,
        }
    }

    pub async fn solver_capability(&self) -> SolverCapability {
        let guard = self.solver_url.load();
        match guard.as_deref() {
            Some(url) if !url.trim().is_empty() => self.probe_solver_capability(url).await,
            _ => SolverCapability::NotConfigured,
        }
    }

    fn solver_route_key(source_key: Option<&str>, url: &str) -> String {
        let source = source_key.unwrap_or("__default__");
        let domain = url
            .parse::<url::Url>()
            .ok()
            .and_then(|url| url.host_str().map(base_domain))
            .unwrap_or_else(|| url.to_string());
        format!("{source}\0{domain}")
    }

    fn solver_session_id(key: &str) -> String {
        use sha2::{Digest, Sha256};

        let digest = Sha256::digest(key.as_bytes());
        format!("kani-{digest:x}")
    }

    pub async fn browser_challenge_credentials(
        &self,
        url: &str,
        force_refresh: bool,
    ) -> Result<BrowserChallengeCredentials> {
        let base = url
            .parse::<url::Url>()
            .ok()
            .and_then(|url| url.host_str().map(base_domain))
            .unwrap_or_else(|| url.to_string());
        let mutex = self
            .solving
            .entry(base.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _guard = mutex.lock().await;

        if force_refresh {
            self.credentials.rcu(|old| {
                let mut credentials = (**old).clone();
                credentials.remove(&base);
                Arc::new(credentials)
            });
        } else if let Some(credentials) = self.credentials.load().get(&base)
            && !credentials.cookies.is_empty()
        {
            return Ok(BrowserChallengeCredentials {
                cookie_header: credentials.cookies.clone(),
                user_agent: credentials.user_agent.clone().unwrap_or_default(),
                from_cache: true,
            });
        }

        let (cookie_header, user_agent) = self.solve_challenge(url, None).await?;
        if cookie_header.is_empty() || user_agent.is_empty() {
            return Err(crate::error::Error::Other(
                "FlareSolverr returned incomplete browser credentials".into(),
            ));
        }
        self.store_credentials(url, &cookie_header, &user_agent);
        Ok(BrowserChallengeCredentials {
            cookie_header,
            user_agent,
            from_cache: false,
        })
    }

    /// Runs `init_script` inside the solver's own cleared browser and returns
    /// the value the page hands to `passPayload`. Cloudflare ties clearance to
    /// the visitor and device, so replaying the solver's cookies into Kani's
    /// Puppeteer is rejected; the capture has to happen where the solve did.
    pub async fn solver_capture(
        &self,
        url: &str,
        init_script: &str,
        timeout_ms: u32,
        source_key: Option<&str>,
        auto_scroll: bool,
    ) -> std::result::Result<String, SolverCaptureError> {
        use std::sync::atomic::Ordering;

        if self.solver_capture_support.load(Ordering::Relaxed) == 2 {
            return Err(SolverCaptureError::Unsupported);
        }
        let guard = self.solver_url.load();
        let solver_url = guard
            .as_deref()
            .ok_or_else(|| SolverCaptureError::Failed("No solver URL configured".into()))?;

        let solver_max_timeout =
            u64::from(timeout_ms).saturating_add(SOLVER_CAPTURE_SOLVE_HEADROOM_MS);
        let transport_timeout = self
            .timings
            .solver_timeout
            .max(std::time::Duration::from_millis(
                solver_max_timeout.saturating_add(SOLVER_CAPTURE_TRANSPORT_BUFFER_MS),
            ));
        let client = self
            .solver_http()
            .map_err(|error| SolverCaptureError::Failed(error.to_string()))?;

        let session_key = source_key.map(|_| Self::solver_route_key(source_key, url));
        let session_id = session_key.as_deref().map(Self::solver_session_id);
        if self.solver_capture_support.load(Ordering::Relaxed) == 0 {
            match self.probe_solver_capability(solver_url).await {
                SolverCapability::Basic => return Err(SolverCaptureError::Unsupported),
                SolverCapability::Unauthorized => return Err(SolverCaptureError::Unauthorized),
                SolverCapability::NotConfigured | SolverCapability::Unreachable => {
                    return Err(SolverCaptureError::Unreachable);
                }
                SolverCapability::Capture => {}
            }
        }
        if self.solver_capture_support.load(Ordering::Relaxed) == 2 {
            return Err(SolverCaptureError::Unsupported);
        }

        let use_session =
            session_id.is_some() && self.solver_capture_support.load(Ordering::Relaxed) != 1;

        let mut body = json!({
            "cmd": "kani.capture",
            "url": url,
            "initScript": init_script,
            "captureTimeout": timeout_ms,
            "autoScroll": auto_scroll,
            "maxTimeout": solver_max_timeout,
        });
        if use_session {
            body["session"] = json!(session_id.as_deref());
            body["session_ttl_minutes"] = json!(SOLVER_SESSION_TTL_MINUTES);
            body["profileKey"] = json!(session_id.as_deref());
        }

        let response = Self::solver_request_with(client, solver_url, Some(transport_timeout))
            .body(body.to_string())
            .send()
            .await
            .map_err(|error| SolverCaptureError::Failed(error.to_string()))?;

        let status = response.status();
        if status.as_u16() == 401 {
            self.solver_capture_support.store(0, Ordering::Relaxed);
            return Err(SolverCaptureError::Unauthorized);
        }
        let response: serde_json::Value = response.json().await.map_err(|error| {
            SolverCaptureError::Failed(format!(
                "FlareSolverr returned HTTP {status} with an unreadable body: {error}"
            ))
        })?;

        let message = response["message"].as_str().unwrap_or("no message");
        if response["status"].as_str().unwrap_or("") != "ok" {
            if message.contains("passPayload was not called") {
                return Err(SolverCaptureError::ScriptProducedNothing(
                    message.to_string(),
                ));
            }
            return Err(SolverCaptureError::Failed(format!(
                "FlareSolverr capture failed: {message}"
            )));
        }

        let payload = response["solution"]["payload"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                SolverCaptureError::Failed("FlareSolverr capture returned no payload".to_string())
            })?;

        // Where a capture's wall-clock went: navigate, solve, reload, poll.
        // Behind the same operator toggle as script-engine request logging, so
        // an instance does not narrate every capture by default.
        if crate::v8_process::v8_debug_logging_enabled() {
            let solution = &response["solution"];
            tracing::info!(
                url,
                session = use_session,
                timings = %solution["timings"],
                rechallenged = solution["reChallenged"].as_bool().unwrap_or(false),
                payload_bytes = payload.len(),
                "solver capture completed"
            );
        }

        self.solver_capture_support
            .store(if use_session { 3 } else { 1 }, Ordering::Relaxed);
        if use_session && let Some(key) = session_key.as_ref() {
            self.solver_sessions
                .insert(key.clone(), std::time::Instant::now());
        }
        Ok(payload)
    }

    async fn destroy_solver_session_keys(&self, keys: Vec<String>) -> usize {
        let solver_url = self.solver_url.load_full();
        let Some(solver_url) = solver_url.as_ref() else {
            return 0;
        };
        let control_timeout = std::time::Duration::from_secs(SOLVER_SESSION_CONTROL_TIMEOUT_SECS);
        let client = match self.solver_http() {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "failed to build solver session control client");
                return 0;
            }
        };
        let mut destroyed = 0;
        for key in keys {
            let session = Self::solver_session_id(&key);
            let result = Self::solver_request_with(client, solver_url, Some(control_timeout))
                .body(
                    json!({
                        "cmd": "sessions.destroy",
                        "session": session,
                    })
                    .to_string(),
                )
                .send()
                .await;
            match result {
                Ok(response) if response.status().is_success() => {}
                Ok(response) => {
                    tracing::warn!(status = %response.status(), %session, "solver session destroy failed");
                }
                Err(error) => {
                    tracing::warn!(%error, %session, "solver session destroy failed");
                }
            }
            self.solver_sessions.remove(&key);
            destroyed += 1;
        }
        destroyed
    }

    /// Destroys every solver browser session owned by one source.
    pub async fn destroy_solver_sessions(&self, source_key: &str) -> usize {
        let prefix = format!("{source_key}\0");
        let keys = self
            .solver_sessions
            .iter()
            .filter(|entry| entry.key().starts_with(&prefix))
            .map(|entry| entry.key().clone())
            .collect();
        self.destroy_solver_session_keys(keys).await
    }

    /// Destroys solver browser sessions for one source after their idle timeout.
    pub async fn reap_solver_sessions(
        &self,
        source_key: &str,
        idle_for: std::time::Duration,
    ) -> usize {
        let prefix = format!("{source_key}\0");
        let keys = self
            .solver_sessions
            .iter()
            .filter(|entry| entry.key().starts_with(&prefix) && entry.value().elapsed() >= idle_for)
            .map(|entry| entry.key().clone())
            .collect();
        self.destroy_solver_session_keys(keys).await
    }

    async fn get_rendered_page(&self, url: &str) -> Result<SmartResponse> {
        let guard = self.solver_url.load();
        let solver_url = guard
            .as_deref()
            .ok_or_else(|| crate::error::Error::Other("No solver URL configured".into()))?;

        let client = self.solver_http()?;

        let body = json!({
          "cmd": "request.get",
          "url": url,
          "maxTimeout": 60000
        });

        let response = Self::solver_request(client, solver_url)
            .body(body.to_string())
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(crate::error::Error::Other(format!(
                "FlareSolverr returned HTTP {}: check your solver URL is correct (should end in /v1)",
                response.status()
            )));
        }

        let response: serde_json::Value = response.json().await?;

        let status = response["status"].as_str().unwrap_or("");
        if status != "ok" {
            return Err(crate::error::Error::Other(format!(
                "FlareSolverr challenge failed with status '{}': {}",
                status,
                response["message"].as_str().unwrap_or("no message")
            )));
        }

        let html = response["solution"]["response"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        let mut headers = rquest::header::HeaderMap::new();
        headers.insert(
            rquest::header::CONTENT_TYPE,
            rquest::header::HeaderValue::from_static("text/html"),
        );

        Ok(SmartResponse::Buffered {
            status: rquest::StatusCode::OK,
            url: url
                .parse::<rquest::Uri>()
                .map_err(|e| crate::error::Error::Other(e.to_string()))?,
            headers,
            body: bytes::Bytes::from(html),
        })
    }

    async fn get_rendered_page_once(&self, url: &str) -> Result<SmartResponse> {
        let base = url
            .parse::<url::Url>()
            .ok()
            .and_then(|u| u.host_str().map(base_domain))
            .unwrap_or_else(|| url.to_string());

        let mutex = self
            .solving
            .entry(base.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();

        let _guard = mutex.lock().await;

        let creds = self.credentials.load();
        if let Some(c) = creds.get(&base)
            && !c.cookies.is_empty()
        {
            tracing::debug!(
                "Challenge for {} already solved, retrying with stored credentials",
                base
            );

            let mut builder = self.client.get(url);
            if let Ok(val) = rquest::header::HeaderValue::from_str(&c.cookies) {
                builder = builder.header(rquest::header::COOKIE, val);
            }

            if let Some(ref ua) = c.user_agent
                && let Ok(val) = rquest::header::HeaderValue::from_str(ua)
            {
                builder = builder.header(rquest::header::USER_AGENT, val);
            }

            let request = builder
                .build()
                .map_err(|e| crate::error::Error::Other(e.to_string()))?;
            let resp = self.client.execute(request).await?;
            return Ok(SmartResponse::Normal(resp));
        }

        tracing::info!("Solving HTML challenge for domain {}", base);
        let result = self.get_rendered_page(url).await?;

        if let Ok((cookies, ua)) = self.solve_challenge(url, None).await {
            self.store_credentials(url, &cookies, &ua);
        }

        Ok(result)
    }

    fn store_credentials(&self, url: &str, cookies: &str, user_agent: &str) {
        let domain = match url
            .parse::<url::Url>()
            .ok()
            .and_then(|u| u.host_str().map(base_domain))
        {
            Some(d) => d,
            None => {
                tracing::warn!(
                    "store_credentials: could not extract domain from '{}', \
                    credentials will not be applied",
                    url
                );
                return;
            }
        };

        let entry = CachedCredentials {
            cookies: cookies.to_string(),
            user_agent: Some(user_agent.to_string()),
            stored_at: Some(std::time::Instant::now()),
            challenge_url: Some(url.to_string()),
        };
        // rcu (not load→clone→store): a blind store would clobber a concurrent
        // update for a different domain. The retry-loop CAS preserves both.
        self.credentials.rcu(|old| {
            let mut m = (**old).clone();
            m.insert(domain.clone(), entry.clone());
            Arc::new(m)
        });
    }

    pub async fn refresh_expiring_credentials(&self) {
        if self.solver_url.load().is_none() {
            return;
        }

        const REFRESH_THRESHOLD_SECS: u64 = 300;

        let domains_to_refresh: Vec<(String, String)> = {
            let creds = self.credentials.load();
            creds
                .iter()
                .filter_map(|(domain, cred)| {
                    let elapsed = cred.stored_at?.elapsed();
                    let expiring_soon = elapsed
                        + std::time::Duration::from_secs(REFRESH_THRESHOLD_SECS)
                        >= self.timings.credential_ttl;
                    let url = cred.challenge_url.clone()?;
                    if expiring_soon {
                        Some((domain.clone(), url))
                    } else {
                        None
                    }
                })
                .collect()
        };

        for (domain, url) in domains_to_refresh {
            tracing::info!("Proactively refreshing credentials for {}", domain);
            match self.solve_challenge_once(&url, None).await {
                Ok((cookies, ua)) => self.store_credentials(&url, &cookies, &ua),
                Err(e) => {
                    tracing::warn!("Proactive credential refresh failed for {}: {}", domain, e)
                }
            }
        }
    }

    /// Warns when the solver secret and the captured payload would cross a
    /// routable network in the clear. The payload matters more than the key:
    /// signing would protect the credential but leaves the extension script
    /// and the captured data readable, which only TLS fixes.
    pub(crate) fn solver_transport_warning(&self) -> Option<&'static str> {
        let guard = self.solver_url.load();
        let url = guard.as_deref()?;
        solver_transport_is_exposed(url).then_some(
            "the solver URL uses plain HTTP to a routable host, so the key and captured \
             payloads cross the network in the clear; put it behind HTTPS or keep it on a \
             private network",
        )
    }

    pub async fn update_solver_url(&self, url: Option<String>) {
        let keys = self
            .solver_sessions
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        self.destroy_solver_session_keys(keys).await;
        self.solver_url.store(Arc::new(url));
        if let Some(warning) = self.solver_transport_warning() {
            tracing::warn!("{warning}");
        }
        self.solver_capture_support
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

pub async fn collect_bytes_limited<S>(
    stream: S,
    content_length_hint: Option<usize>,
    max_bytes: usize,
) -> Result<bytes::Bytes>
where
    S: TryStream<Ok = bytes::Bytes> + Unpin,
    S::Error: Into<crate::error::Error>,
{
    let mut stream = stream;

    if let Some(len) = content_length_hint
        && len > max_bytes
    {
        return Err(crate::error::Error::Other(format!(
            "Content-Length {} exceeds limit {}",
            len, max_bytes
        )));
    }

    let mut buf = bytes::BytesMut::new();
    let mut received = 0usize;

    while let Some(chunk) = stream.try_next().await.map_err(Into::into)? {
        received += chunk.len();
        if received > max_bytes {
            return Err(crate::error::Error::Other(format!(
                "body exceeded limit of {} bytes mid-stream",
                max_bytes
            )));
        }
        buf.extend_from_slice(&chunk);
    }

    Ok(buf.freeze())
}

fn base_domain(host: &str) -> String {
    use publicsuffix::{List, Psl};
    static LIST: std::sync::OnceLock<List> = std::sync::OnceLock::new();
    let list = LIST.get_or_init(List::default);

    list.domain(host.as_bytes())
        .and_then(|d| {
            std::str::from_utf8(d.as_bytes())
                .ok()
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| host.to_string())
}

/// Test-only constructor: uses Chrome emulation (for HTTP/1.1 compatibility with wiremock) but
/// omits ValidatingResolver (which blocks 127.0.0.1) and the per-request timeout (which
/// interacts badly with `#[tokio::test(start_paused = true)]`).
#[cfg(test)]
impl SmartClient {
    pub fn new_for_test() -> Result<Self> {
        let client = rquest::Client::builder()
            .emulation(rquest_util::Emulation::Chrome130)
            .redirect(rquest::redirect::Policy::none())
            .build()?;
        let (circuit_event_tx, _) = tokio::sync::broadcast::channel(8);
        Ok(Self {
            client,
            credentials: Arc::new(ArcSwap::from_pointee(HashMap::new())),
            solver_url: Arc::new(ArcSwap::from_pointee(None)),
            solver_sessions: Arc::new(dashmap::DashMap::new()),
            solver_capture_support: Arc::new(std::sync::atomic::AtomicU8::new(0)),
            solver_client: Arc::new(std::sync::OnceLock::new()),
            solving: Arc::new(dashmap::DashMap::new()),
            host_circuits: Arc::new(dashmap::DashMap::new()),
            rate_states: Arc::new(dashmap::DashMap::new()),
            circuit_event_tx,
            cond_cache: Arc::new(ConditionalGetCache::new()),
            timings: Timings::default(),
            budgets: Budgets::default(),
            allow_loopback_egress: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            local_grants: Arc::default(),
            follows_redirects: false,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn not_retryable_on_429() {
        assert!(!is_retryable(rquest::StatusCode::TOO_MANY_REQUESTS));
    }

    #[test]
    fn retryable_on_502() {
        assert!(is_retryable(rquest::StatusCode::BAD_GATEWAY));
    }

    #[test]
    fn retryable_on_504() {
        assert!(is_retryable(rquest::StatusCode::GATEWAY_TIMEOUT));
    }

    #[test]
    fn not_retryable_on_200() {
        assert!(!is_retryable(rquest::StatusCode::OK));
    }

    #[test]
    fn not_retryable_on_400() {
        assert!(!is_retryable(rquest::StatusCode::BAD_REQUEST));
    }

    #[test]
    fn not_retryable_on_403() {
        assert!(!is_retryable(rquest::StatusCode::FORBIDDEN));
    }

    #[test]
    fn not_retryable_on_503() {
        assert!(!is_retryable(rquest::StatusCode::SERVICE_UNAVAILABLE));
    }

    #[test]
    fn no_header_gives_exponential_backoff() {
        let d0 = compute_delay(None, 0, BASE_DELAY);
        let d1 = compute_delay(None, 1, BASE_DELAY);
        let d2 = compute_delay(None, 2, BASE_DELAY);
        assert_eq!(d0, BASE_DELAY);
        assert_eq!(d1, BASE_DELAY * 2);
        assert_eq!(d2, BASE_DELAY * 4);
    }

    #[test]
    fn retry_after_integer_seconds_used() {
        let mut headers = rquest::header::HeaderMap::new();
        headers.insert(
            rquest::header::RETRY_AFTER,
            rquest::header::HeaderValue::from_static("10"),
        );
        let d = compute_delay(Some(&headers), 0, BASE_DELAY);
        assert_eq!(d, std::time::Duration::from_secs(10));
    }

    #[test]
    fn retry_after_capped_at_max() {
        let mut headers = rquest::header::HeaderMap::new();
        headers.insert(
            rquest::header::RETRY_AFTER,
            rquest::header::HeaderValue::from_static("9999"),
        );
        let d = compute_delay(Some(&headers), 0, BASE_DELAY);
        assert_eq!(d, std::time::Duration::from_secs(RETRY_AFTER_CAP_SECS));
    }

    #[test]
    fn empty_headers_falls_back_to_backoff() {
        let headers = rquest::header::HeaderMap::new();
        let d = compute_delay(Some(&headers), 0, BASE_DELAY);
        assert_eq!(d, BASE_DELAY);
    }

    #[test]
    fn a_rate_limit_registered_on_a_subdomain_is_found_by_the_apex_lookup() {
        let client = SmartClient::new_for_test().unwrap();
        let cfg = kani_shared::extension::RateLimitConfig {
            requests_per_second: 1.0,
            burst: 1,
            max_concurrent: 1,
            ..Default::default()
        };
        client.register_rate_limit("api.example.com", &cfg);
        assert!(
            client.rate_states.get("example.com").is_some(),
            "a limit registered on a subdomain must be findable under the key \
             requests actually use, or it silently governs nothing"
        );
        client.deregister_rate_limit("api.example.com");
        assert!(client.rate_states.get("example.com").is_none());
    }

    #[test]
    fn base_domain_strips_subdomain() {
        let d = base_domain("sub.example.com");
        assert_eq!(d, "example.com");
    }

    #[test]
    fn base_domain_preserves_apex() {
        let d = base_domain("example.com");
        assert_eq!(d, "example.com");
    }

    #[test]
    fn base_domain_is_deterministic_for_ips() {
        let first = base_domain("8.8.8.8");
        let second = base_domain("8.8.8.8");
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn smart_response_buffered_accessors() {
        let mut headers = rquest::header::HeaderMap::new();
        headers.insert(
            rquest::header::CONTENT_TYPE,
            rquest::header::HeaderValue::from_static("application/json"),
        );
        let resp = SmartResponse::Buffered {
            status: rquest::StatusCode::OK,
            url: "https://example.com/test".parse::<rquest::Uri>().unwrap(),
            headers,
            body: bytes::Bytes::from("hello world"),
        };
        assert_eq!(resp.status(), rquest::StatusCode::OK);
        assert_eq!(resp.url().to_string(), "https://example.com/test");
        assert!(resp.headers().contains_key(rquest::header::CONTENT_TYPE));
    }

    #[tokio::test]
    async fn smart_response_buffered_text() {
        let resp = SmartResponse::Buffered {
            status: rquest::StatusCode::OK,
            url: "https://example.com/".parse::<rquest::Uri>().unwrap(),
            headers: rquest::header::HeaderMap::new(),
            body: bytes::Bytes::from("test body"),
        };
        let text = resp.text().await.unwrap();
        assert_eq!(text, "test body");
    }

    #[tokio::test]
    async fn smart_response_buffered_bytes() {
        let resp = SmartResponse::Buffered {
            status: rquest::StatusCode::NOT_FOUND,
            url: "https://example.com/".parse::<rquest::Uri>().unwrap(),
            headers: rquest::header::HeaderMap::new(),
            body: bytes::Bytes::from_static(b"\x01\x02\x03"),
        };
        let b = resp.bytes().await.unwrap();
        assert_eq!(&b[..], b"\x01\x02\x03");
    }

    #[tokio::test]
    async fn circuit_starts_closed() {
        let client = SmartClient::new(None).unwrap();
        assert!(!client.is_circuit_open("example.com"));
    }

    #[tokio::test]
    async fn circuit_opens_after_threshold_failures() {
        let client = SmartClient::new(None).unwrap();
        for _ in 0..CIRCUIT_OPEN_THRESHOLD {
            client.record_failure("example.com");
        }
        assert!(client.is_circuit_open("example.com"));
    }

    #[tokio::test]
    async fn record_success_resets_failure_counter() {
        let client = SmartClient::new(None).unwrap();
        for _ in 0..CIRCUIT_OPEN_THRESHOLD - 1 {
            client.record_failure("example.com");
        }
        client.record_success("example.com");
        client.record_failure("example.com");
        assert!(!client.is_circuit_open("example.com"));
    }

    #[tokio::test]
    async fn record_success_on_unknown_domain_is_noop() {
        let client = SmartClient::new(None).unwrap();
        client.record_success("never-seen.com");
        assert!(!client.is_circuit_open("never-seen.com"));
    }

    #[tokio::test]
    async fn collect_bytes_limited_rejects_large_content_length() {
        use futures::stream;
        let small_chunk = bytes::Bytes::from("hello");
        let s = stream::iter(vec![Ok::<_, crate::error::Error>(small_chunk)]);
        let err = collect_bytes_limited(s, Some(1000), 10).await.unwrap_err();
        assert!(err.to_string().contains("exceeds limit"));
    }

    #[tokio::test]
    async fn collect_bytes_limited_rejects_oversized_body() {
        use futures::stream;
        let big = bytes::Bytes::from(vec![0u8; 20]);
        let s = stream::iter(vec![Ok::<_, crate::error::Error>(big)]);
        let err = collect_bytes_limited(s, None, 10).await.unwrap_err();
        assert!(err.to_string().contains("exceeded limit"));
    }

    #[tokio::test]
    async fn collect_bytes_limited_succeeds_within_limit() {
        use futures::stream;
        let data = bytes::Bytes::from("hello");
        let s = stream::iter(vec![Ok::<_, crate::error::Error>(data)]);
        let result = collect_bytes_limited(s, None, 100).await.unwrap();
        assert_eq!(&result[..], b"hello");
    }

    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn solver_with(capabilities: Option<serde_json::Value>, v1_status: u16) -> MockServer {
        let server = MockServer::start().await;
        let mut index = serde_json::json!({ "msg": "ready", "version": "3.5.0" });
        if let Some(caps) = capabilities {
            index["capabilities"] = caps;
        }
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(index))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1"))
            .respond_with(
                ResponseTemplate::new(v1_status).set_body_json(serde_json::json!({
                    "status": "ok", "sessions": []
                })),
            )
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn probe_reports_capture_when_sessions_are_advertised() {
        let server = solver_with(
            Some(serde_json::json!(["kani.capture/1", "kani.capture/2"])),
            200,
        )
        .await;
        let client = SmartClient::new(Some(server.uri() + "/v1")).unwrap();

        assert_eq!(
            client
                .probe_solver_capability(&(server.uri() + "/v1"))
                .await,
            SolverCapability::Capture
        );
        assert_eq!(
            client
                .solver_capture_support
                .load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    }

    #[tokio::test]
    async fn probe_reports_capture_without_sessions_for_version_one_only() {
        let server = solver_with(Some(serde_json::json!(["kani.capture/1"])), 200).await;
        let client = SmartClient::new(Some(server.uri() + "/v1")).unwrap();

        assert_eq!(
            client
                .probe_solver_capability(&(server.uri() + "/v1"))
                .await,
            SolverCapability::Capture
        );
        assert_eq!(
            client
                .solver_capture_support
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }

    #[tokio::test]
    async fn a_stock_solver_without_capabilities_probes_as_basic() {
        let server = solver_with(None, 200).await;
        let client = SmartClient::new(Some(server.uri() + "/v1")).unwrap();

        assert_eq!(
            client
                .probe_solver_capability(&(server.uri() + "/v1"))
                .await,
            SolverCapability::Basic
        );
        assert_eq!(
            client
                .solver_capture_support
                .load(std::sync::atomic::Ordering::Relaxed),
            2
        );
    }

    #[tokio::test]
    async fn an_unrelated_capability_list_still_probes_as_basic() {
        let server = solver_with(Some(serde_json::json!(["something.else/1"])), 200).await;
        let client = SmartClient::new(Some(server.uri() + "/v1")).unwrap();

        assert_eq!(
            client
                .probe_solver_capability(&(server.uri() + "/v1"))
                .await,
            SolverCapability::Basic
        );
    }

    #[tokio::test]
    async fn a_rejected_key_probes_as_unauthorized_and_does_not_cache() {
        let server = solver_with(Some(serde_json::json!(["kani.capture/2"])), 401).await;
        let client = SmartClient::new(Some(server.uri() + "/v1")).unwrap();

        assert_eq!(
            client
                .probe_solver_capability(&(server.uri() + "/v1"))
                .await,
            SolverCapability::Unauthorized
        );
        assert_eq!(
            client
                .solver_capture_support
                .load(std::sync::atomic::Ordering::Relaxed),
            0,
            "a rejected key says nothing about the image's capabilities"
        );
    }

    #[tokio::test]
    async fn an_absent_solver_probes_as_unreachable() {
        let client = SmartClient::new(Some("http://127.0.0.1:1/v1".into())).unwrap();
        assert_eq!(
            client
                .probe_solver_capability("http://127.0.0.1:1/v1")
                .await,
            SolverCapability::Unreachable
        );
    }

    #[tokio::test]
    async fn changing_the_solver_url_forces_a_re_probe() {
        let server = solver_with(Some(serde_json::json!(["kani.capture/2"])), 200).await;
        let client = SmartClient::new(Some(server.uri() + "/v1")).unwrap();
        client
            .probe_solver_capability(&(server.uri() + "/v1"))
            .await;
        assert_eq!(
            client
                .solver_capture_support
                .load(std::sync::atomic::Ordering::Relaxed),
            3
        );

        client
            .update_solver_url(Some("http://example.invalid/v1".into()))
            .await;
        assert_eq!(
            client
                .solver_capture_support
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn the_index_url_is_derived_from_the_command_url() {
        assert_eq!(
            SmartClient::solver_index_url("http://solver:8191/v1").as_deref(),
            Some("http://solver:8191/")
        );
    }

    #[test]
    fn a_private_or_local_solver_is_not_flagged() {
        for url in [
            "http://127.0.0.1:8191/v1",
            "http://localhost:8191/v1",
            "http://flaresolverr:8191/v1",
            "http://192.168.1.50:8191/v1",
            "http://10.0.0.4:8191/v1",
            "http://172.16.5.9:8191/v1",
            "http://nas.local:8191/v1",
            "http://solver.lan:8191/v1",
            "http://[::1]:8191/v1",
            "https://solver.example.com/v1",
        ] {
            assert!(
                !solver_transport_is_exposed(url),
                "{url} should not warn: it is private, local, or encrypted"
            );
        }
    }

    #[test]
    fn plain_http_to_a_routable_host_is_flagged() {
        for url in [
            "http://solver.example.com/v1",
            "http://solver.example.com:8191/v1",
            "http://203.0.113.10:8191/v1",
        ] {
            assert!(
                solver_transport_is_exposed(url),
                "{url} should warn: key and payload cross a routable network in the clear"
            );
        }
    }

    #[test]
    fn empty_solver_url_is_not_configured() {
        let client = SmartClient::new(Some("   ".into())).unwrap();
        assert!(!client.solver_configured());
    }

    #[test]
    fn solver_session_ids_are_stable_and_source_scoped() {
        let first = SmartClient::solver_route_key(Some("source-a"), "https://sub.example.com/a");
        let same = SmartClient::solver_route_key(Some("source-a"), "https://example.com/b");
        let other = SmartClient::solver_route_key(Some("source-b"), "https://example.com/a");

        assert_eq!(
            SmartClient::solver_session_id(&first),
            SmartClient::solver_session_id(&same)
        );
        assert_ne!(
            SmartClient::solver_session_id(&first),
            SmartClient::solver_session_id(&other)
        );
        assert_eq!(SmartClient::solver_session_id(&first).len(), 69);
    }

    #[tokio::test]
    async fn a_script_that_never_submits_is_distinguished_from_a_broken_solver() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "msg": "ready",
                "capabilities": ["kani.capture/1", "kani.capture/2"]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({"cmd": "kani.capture"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "error",
                "message": "passPayload was not called within 1000 ms."
            })))
            .mount(&server)
            .await;
        let client = SmartClient::new(Some(server.uri())).unwrap();

        let error = client
            .solver_capture(
                "https://sub.example.com/a",
                "// never submits",
                1000,
                None,
                false,
            )
            .await
            .expect_err("a capture that never submits must fail");

        assert!(
            matches!(error, SolverCaptureError::ScriptProducedNothing(_)),
            "a healthy solver running a script that never submits must not look \
             like a broken solver, got: {error:?}"
        );
    }

    #[tokio::test]
    async fn solver_capture_owns_and_destroys_a_source_session() {
        let server = MockServer::start().await;
        let key = SmartClient::solver_route_key(Some("source-a"), "https://sub.example.com/a");
        let session = SmartClient::solver_session_id(&key);
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "msg": "ready",
                "capabilities": ["kani.capture/1", "kani.capture/2"]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({
                "cmd": "kani.capture",
                "session": session,
                "session_ttl_minutes": SOLVER_SESSION_TTL_MINUTES,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok",
                "solution": {"payload": "captured"}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({
                "cmd": "sessions.destroy",
                "session": session,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = SmartClient::new(Some(server.uri())).unwrap();

        let payload = client
            .solver_capture(
                "https://sub.example.com/a",
                "passPayload('captured')",
                1000,
                Some("source-a"),
                false,
            )
            .await
            .expect("capture");
        assert_eq!(payload, "captured");
        assert!(client.solver_sessions.contains_key(&key));

        assert_eq!(client.destroy_solver_sessions("source-a").await, 1);
        assert!(!client.solver_sessions.contains_key(&key));
    }

    #[tokio::test]
    async fn browser_clearance_is_cached_and_force_refresh_invalidates_it() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok",
                "solution": {
                    "userAgent": "solver-agent",
                    "cookies": [{"name": "cf_clearance", "value": "clearance"}]
                }
            })))
            .mount(&server)
            .await;
        let client = SmartClient::new(Some(server.uri())).unwrap();

        let fresh = client
            .browser_challenge_credentials("https://sub.example.com/browse", false)
            .await
            .expect("fresh credentials");
        assert!(!fresh.from_cache);
        assert_eq!(fresh.cookie_header, "cf_clearance=clearance");
        let cached = client
            .browser_challenge_credentials("https://example.com/next", false)
            .await
            .expect("cached credentials");
        assert!(cached.from_cache);
        let refreshed = client
            .browser_challenge_credentials("https://example.com/next", true)
            .await
            .expect("refreshed credentials");
        assert!(!refreshed.from_cache);

        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn get_request_reaches_server() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/hello"))
            .respond_with(ResponseTemplate::new(200).set_body_string("world"))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .get(format!("{}/hello", server.uri()))
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::OK);
        let body = resp.text().await.unwrap();
        assert_eq!(body, "world");
    }

    #[tokio::test]
    async fn post_request_json_body_reaches_server() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .and(body_partial_json(serde_json::json!({"k": "v"})))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .post(format!("{}/api", server.uri()))
            .json(&serde_json::json!({"k": "v"}))
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 201);
    }

    #[tokio::test]
    async fn post_request_form_body_reaches_server() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/form"))
            .and(header("content-type", "application/x-www-form-urlencoded"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .post(format!("{}/form", server.uri()))
            .form(&[("field", "val")])
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::OK);
    }

    #[tokio::test]
    async fn custom_header_forwarded_to_server() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/h"))
            .and(header("x-custom", "test-val"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .get(format!("{}/h", server.uri()))
            .header("x-custom", "test-val")
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::OK);
    }

    #[tokio::test]
    async fn query_params_forwarded_to_server() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("q", "manga"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .get(format!("{}/search?q=manga", server.uri()))
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::OK);
    }

    #[tokio::test]
    async fn html_response_is_buffered() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(b"<html><body>hello</body></html>".to_vec(), "text/html"),
            )
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .get(format!("{}/page", server.uri()))
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert!(
            matches!(resp, SmartResponse::Buffered { .. }),
            "expected Buffered for text/html"
        );
        let body = resp.text().await.unwrap();
        assert!(body.contains("hello"));
    }

    #[tokio::test]
    async fn json_response_is_normal() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/data"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(r#"{"ok":true}"#),
            )
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .get(format!("{}/data", server.uri()))
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert!(matches!(resp, SmartResponse::Normal(_)));
    }

    #[tokio::test]
    async fn not_found_404_returns_normal_not_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .get(format!("{}/missing", server.uri()))
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn server_error_500_returned_as_normal() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/err"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .get(format!("{}/err", server.uri()))
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn an_html_body_over_the_budget_is_refused_not_buffered() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/huge"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw("x".repeat(64 * 1024).into_bytes(), "text/html"),
            )
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap().with_budgets(Budgets {
            max_html_body_bytes: 1024,
            ..Budgets::default()
        });
        let req = client
            .inner()
            .get(format!("{}/huge", server.uri()))
            .build()
            .unwrap();

        let err = match client.send_request(req).await {
            Ok(_) => panic!("a body past the ceiling must be refused"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("limit") || err.to_string().contains("exceeds"),
            "unhelpful error: {err}"
        );
    }

    #[tokio::test]
    async fn an_html_body_within_the_budget_is_still_served() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/small"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "<html><body>ok</body></html>".as_bytes().to_vec(),
                "text/html",
            ))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap().with_budgets(Budgets {
            max_html_body_bytes: 1024,
            ..Budgets::default()
        });
        let req = client
            .inner()
            .get(format!("{}/small", server.uri()))
            .build()
            .unwrap();

        let resp = client.send_request(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[test]
    fn the_budget_seam_overrides_every_ceiling() {
        let client = SmartClient::new_for_test().unwrap().with_budgets(Budgets {
            max_html_body_bytes: 1,
            max_http_response_bytes: 2,
            max_option_set_bytes: 3,
        });
        assert_eq!(client.budgets().max_html_body_bytes, 1);
        assert_eq!(client.budgets().max_http_response_bytes, 2);
        assert_eq!(client.budgets().max_option_set_bytes, 3);

        let default = SmartClient::new_for_test().unwrap();
        assert_eq!(
            default.budgets(),
            Budgets::default(),
            "a client that was never overridden must carry the shipped ceilings"
        );
    }

    #[tokio::test]
    async fn a_429_is_returned_immediately_without_retrying() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rl"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
            .expect(1)
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let req = client
            .inner()
            .get(format!("{}/rl", server.uri()))
            .build()
            .unwrap();
        let resp = client.send_request(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 429);
    }

    #[tokio::test]
    async fn safe_get_returns_a_429_immediately_without_retrying() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rl"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
            .expect(1)
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let resp = client
            .safe_get(&format!("{}/rl", server.uri()), None)
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 429);
    }

    #[tokio::test]
    async fn safe_get_follows_redirect() {
        let server = MockServer::start().await;
        let dest = format!("{}/dest", server.uri());
        Mock::given(method("GET"))
            .and(path("/redir"))
            .respond_with(ResponseTemplate::new(301).insert_header("location", dest.as_str()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/dest"))
            .respond_with(ResponseTemplate::new(200).set_body_string("final"))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let resp = client
            .safe_get(&format!("{}/redir", server.uri()), None)
            .await
            .unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::OK);
    }

    #[tokio::test]
    async fn safe_get_too_many_redirects_returns_error() {
        let server = MockServer::start().await;
        for i in 1..=12 {
            let next = format!("{}/r{}", server.uri(), i + 1);
            Mock::given(method("GET"))
                .and(path(format!("/r{}", i)))
                .respond_with(ResponseTemplate::new(301).insert_header("location", next.as_str()))
                .mount(&server)
                .await;
        }

        let client = SmartClient::new_for_test().unwrap();
        let Err(err) = client.safe_get(&format!("{}/r1", server.uri()), None).await else {
            panic!("expected error for too many redirects");
        };
        let msg = err.to_string();
        assert!(
            msg.contains("too many redirects") || msg.contains("redirect"),
            "{msg}"
        );
    }

    #[tokio::test]
    async fn a_relative_redirect_resolves_against_the_current_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a/redir"))
            .respond_with(ResponseTemplate::new(301).insert_header("location", "../dest"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/dest"))
            .respond_with(ResponseTemplate::new(200).set_body_string("final"))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let resp = client
            .safe_get(&format!("{}/a/redir", server.uri()), None)
            .await
            .unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::OK);
        assert_eq!(
            resp.url().path(),
            "/dest",
            "`../dest` joined against `/a/redir`"
        );
    }

    #[tokio::test]
    async fn a_protocol_relative_redirect_inherits_the_scheme() {
        let server = MockServer::start().await;
        let authority = server
            .uri()
            .strip_prefix("http://")
            .expect("wiremock serves over http")
            .to_string();
        Mock::given(method("GET"))
            .and(path("/redir"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("location", format!("//{authority}/dest").as_str()),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/dest"))
            .respond_with(ResponseTemplate::new(200).set_body_string("final"))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let resp = client
            .safe_get(&format!("{}/redir", server.uri()), None)
            .await
            .unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::OK);
        assert_eq!(
            resp.url().scheme_str(),
            Some("http"),
            "the protocol-relative Location inherited the http scheme"
        );
    }

    #[tokio::test]
    async fn a_redirect_to_a_forbidden_ip_literal_is_refused() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/redir"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("location", "http://169.254.169.254/latest/meta-data/"),
            )
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test()
            .unwrap()
            .with_allow_loopback_egress(true);
        let Err(err) = client
            .safe_get(&format!("{}/redir", server.uri()), None)
            .await
        else {
            panic!("expected the redirect to a forbidden host to be refused");
        };
        assert!(
            err.to_string().contains("redirect to a forbidden host"),
            "refused at the redirect, got: {err}"
        );
    }

    #[tokio::test]
    async fn get_refuses_a_redirect_to_a_forbidden_ip_literal() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/redir"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("location", "http://169.254.169.254/latest/meta-data/"),
            )
            .mount(&server)
            .await;

        let client = SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true);
        let Err(err) = client.get(&format!("{}/redir", server.uri())).await else {
            panic!("a source redirect to a forbidden host must be refused");
        };
        assert!(
            format!("{err:?}").contains("forbidden host"),
            "refused at the redirect, got: {err:?}"
        );
    }

    #[test]
    fn a_grant_exempts_only_its_own_host_from_the_egress_rule() {
        let never = std::sync::atomic::AtomicBool::new(false);
        let grants = crate::network::LocalGrants::parse(&["10.0.0.5:8080".to_string()]).unwrap();
        assert!(!egress_forbidden(&never, &grants, "http://10.0.0.5:8080/x"));
        assert!(egress_forbidden(&never, &grants, "http://10.0.0.5:9090/x"));
        assert!(egress_forbidden(&never, &grants, "http://10.0.0.6:8080/x"));
        assert!(egress_forbidden(&never, &grants, "http://169.254.169.254/"));
        let none = crate::network::LocalGrants::default();
        assert!(egress_forbidden(&never, &none, "http://10.0.0.5:8080/x"));
    }

    #[tokio::test]
    async fn a_granted_client_keeps_refusing_ungranted_private_hosts() {
        let base = SmartClient::new(None).unwrap();
        let granted = base
            .with_local_grants(
                crate::network::LocalGrants::parse(&["10.0.0.5:8080".to_string()]).unwrap(),
            )
            .unwrap();
        let Err(err) = granted.safe_get("http://10.0.0.6:8080/", None).await else {
            panic!("an ungranted private host was reached");
        };
        assert!(err.to_string().contains("forbidden host"), "got: {err}");
        let Err(err) = base.safe_get("http://10.0.0.5:8080/", None).await else {
            panic!("the base client must not inherit the grant");
        };
        assert!(err.to_string().contains("forbidden host"), "got: {err}");
    }

    #[tokio::test]
    async fn a_first_request_to_a_forbidden_ip_literal_is_refused() {
        let client = SmartClient::new(None).unwrap();
        for url in [
            "http://169.254.169.254/latest/meta-data/",
            "http://127.0.0.1:9/",
            "http://10.0.0.1/",
            "http://[::1]:9/",
        ] {
            for (label, result) in [
                ("get", client.get(url).await.err()),
                ("safe_get", client.safe_get(url, None).await.err()),
            ] {
                let err = result.unwrap_or_else(|| panic!("{label} reached {url}"));
                assert!(
                    err.to_string()
                        .contains("request to a forbidden host refused"),
                    "{label} {url} refused for the wrong reason: {err}"
                );
            }
        }
    }

    #[tokio::test]
    async fn the_loopback_test_switch_exempts_loopback_only() {
        let client = SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true);
        for url in [
            "http://169.254.169.254/",
            "http://10.0.0.1/",
            "http://192.168.1.1/",
        ] {
            let Err(err) = client.safe_get(url, None).await else {
                panic!("{url} was reached with only loopback allowed");
            };
            assert!(
                err.to_string()
                    .contains("request to a forbidden host refused"),
                "{url} must stay refused with loopback allowed: {err}"
            );
        }
    }

    #[tokio::test]
    async fn a_redirect_loop_fails_fast_without_amplification() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/loop"))
            .respond_with(ResponseTemplate::new(301).insert_header("location", "/loop"))
            .mount(&server)
            .await;

        let client = SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true);
        let start = std::time::Instant::now();
        let res = client.get(&format!("{}/loop", server.uri())).await;
        let elapsed = start.elapsed();

        assert!(res.is_err(), "a redirect loop is refused at the limit");
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "redirect loop must fail fast, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn a_call_is_bounded_by_the_whole_call_deadline() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap().with_timings(Timings {
            whole_call_deadline: std::time::Duration::from_millis(50),
            retry_base_delay: std::time::Duration::from_millis(100),
            retry_jitter: std::time::Duration::ZERO,
            ..Timings::default()
        });
        let start = std::time::Instant::now();
        let res = client.get(&format!("{}/x", server.uri())).await;
        let elapsed = start.elapsed();

        assert!(res.is_err(), "the call is stopped at its deadline");
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "the deadline bounds cumulative retry time, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn safe_get_redirect_to_non_http_scheme_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/bad"))
            .respond_with(
                ResponseTemplate::new(301).insert_header("location", "ftp://example.com/file"),
            )
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let Err(err) = client
            .safe_get(&format!("{}/bad", server.uri()), None)
            .await
        else {
            panic!("expected error for bad redirect scheme");
        };
        assert!(err.to_string().contains("forbidden scheme"), "{err}");
    }

    #[test]
    fn register_and_deregister_rate_limit() {
        let client = SmartClient::new(None).unwrap();
        let cfg = kani_shared::extension::RateLimitConfig {
            requests_per_second: 10.0,
            burst: 5,
            max_concurrent: 3,
            max_hook_requests: 3,
        };
        client.register_rate_limit("example.com", &cfg);
        assert!(client.rate_states.contains_key("example.com"));
        client.deregister_rate_limit("example.com");
        assert!(!client.rate_states.contains_key("example.com"));
    }

    #[tokio::test]
    async fn rate_limiter_shapes_burst() {
        let client = SmartClient::new(None).unwrap();
        let cfg = kani_shared::extension::RateLimitConfig {
            requests_per_second: 100.0,
            burst: 3,
            max_concurrent: 10,
            max_hook_requests: 3,
        };
        client.register_rate_limit("test.local", &cfg);
        let state = client
            .rate_states
            .get("test.local")
            .map(|r| Arc::clone(&*r))
            .unwrap();

        let mut passed = 0u32;
        let mut blocked = 0u32;
        for _ in 0..6 {
            match state.limiter.check() {
                Ok(_) => passed += 1,
                Err(_) => blocked += 1,
            }
        }
        assert_eq!(passed, 3, "only burst={} requests pass immediately", 3);
        assert_eq!(blocked, 3, "remaining requests are rate-limited");
    }

    #[test]
    fn cond_cache_records_etag_and_applies_if_none_match() {
        let cache = ConditionalGetCache::new();
        let url = "https://example.com/feed";
        let mut headers = rquest::header::HeaderMap::new();
        headers.insert(
            rquest::header::ETAG,
            rquest::header::HeaderValue::from_static("\"abc123\""),
        );
        cache.record(url, &headers);

        let client = rquest::Client::builder()
            .emulation(rquest_util::Emulation::Chrome130)
            .build()
            .unwrap();
        let builder = client.get(url);
        let req = cache.apply_to(url, builder).build().unwrap();
        let val = req.headers().get(rquest::header::IF_NONE_MATCH).unwrap();
        assert_eq!(val, "\"abc123\"");
    }

    #[test]
    fn cond_cache_falls_back_to_last_modified() {
        let cache = ConditionalGetCache::new();
        let url = "https://example.com/page";
        let mut headers = rquest::header::HeaderMap::new();
        headers.insert(
            rquest::header::LAST_MODIFIED,
            rquest::header::HeaderValue::from_static("Wed, 01 Jan 2025 00:00:00 GMT"),
        );
        cache.record(url, &headers);

        let client = rquest::Client::builder()
            .emulation(rquest_util::Emulation::Chrome130)
            .build()
            .unwrap();
        let builder = client.get(url);
        let req = cache.apply_to(url, builder).build().unwrap();
        assert!(req.headers().get(rquest::header::IF_NONE_MATCH).is_none());
        let val = req
            .headers()
            .get(rquest::header::IF_MODIFIED_SINCE)
            .unwrap();
        assert_eq!(val, "Wed, 01 Jan 2025 00:00:00 GMT");
    }

    #[tokio::test]
    async fn safe_get_conditional_records_etag_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/feed"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"v1\"")
                    .set_body_string("data"),
            )
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let resp = client
            .safe_get_conditional(&format!("{}/feed", server.uri()), None)
            .await
            .unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::OK);

        assert!(
            client
                .cond_cache
                .0
                .contains_key(&format!("{}/feed", server.uri())),
            "etag should be recorded after 200"
        );
    }

    #[tokio::test]
    async fn safe_get_conditional_returns_not_modified_on_304() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/resource"))
            .and(header("if-none-match", "\"v1\""))
            .respond_with(ResponseTemplate::new(304))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let url = format!("{}/resource", server.uri());

        let mut seed_headers = rquest::header::HeaderMap::new();
        seed_headers.insert(
            rquest::header::ETAG,
            rquest::header::HeaderValue::from_static("\"v1\""),
        );
        client.cond_cache.record(&url, &seed_headers);

        let resp = client.safe_get_conditional(&url, None).await.unwrap();
        assert!(
            matches!(resp, SmartResponse::NotModified { .. }),
            "304 with cond_cache must return NotModified"
        );
    }

    #[tokio::test]
    async fn safe_get_plain_ignores_304_from_cache() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/resource"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let client = SmartClient::new_for_test().unwrap();
        let url = format!("{}/resource", server.uri());
        let resp = client.safe_get(&url, None).await.unwrap();
        assert_eq!(resp.status(), rquest::StatusCode::OK);
        assert!(
            !client.cond_cache.0.contains_key(&url),
            "safe_get (non-conditional) must not populate cond_cache"
        );
    }

    #[tokio::test]
    async fn circuit_open_emits_event_on_first_open() {
        let client = SmartClient::new(None).unwrap();
        let mut rx = client.subscribe_circuit_events();

        for _ in 0..CIRCUIT_OPEN_THRESHOLD {
            client.record_failure("example.com");
        }

        let ev = rx
            .try_recv()
            .expect("CircuitOpenedEvent should have been sent");
        assert_eq!(ev.host, "example.com");
        assert_eq!(ev.failure_count, CIRCUIT_OPEN_THRESHOLD);
    }

    #[tokio::test]
    async fn circuit_open_does_not_re_emit_while_still_open() {
        let client = SmartClient::new(None).unwrap();
        let mut rx = client.subscribe_circuit_events();

        for _ in 0..CIRCUIT_OPEN_THRESHOLD + 3 {
            client.record_failure("example.com");
        }

        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1, "exactly one event emitted for the open edge");
    }

    #[tokio::test]
    async fn reset_circuit_clears_open_state() {
        let client = SmartClient::new(None).unwrap();
        for _ in 0..CIRCUIT_OPEN_THRESHOLD {
            client.record_failure("example.com");
        }
        assert!(client.is_circuit_open("example.com"));
        client.reset_circuit("example.com");
        assert!(!client.is_circuit_open("example.com"));
    }

    #[tokio::test]
    async fn list_circuits_reflects_state() {
        let client = SmartClient::new(None).unwrap();
        for _ in 0..CIRCUIT_OPEN_THRESHOLD {
            client.record_failure("example.com");
        }
        let circuits = client.list_circuits();
        let entry = circuits
            .iter()
            .find(|v| v["host"] == "example.com")
            .expect("example.com should appear in list");
        assert_eq!(entry["is_open"], serde_json::json!(true));
        assert!(entry["consecutive_failures"].as_u64().unwrap() >= CIRCUIT_OPEN_THRESHOLD as u64);
    }

    #[tokio::test]
    async fn semaphore_caps_concurrency() {
        let client = SmartClient::new(None).unwrap();
        let cfg = kani_shared::extension::RateLimitConfig {
            requests_per_second: 100.0,
            burst: 100,
            max_concurrent: 2,
            max_hook_requests: 3,
        };
        client.register_rate_limit("sem.local", &cfg);
        let state = client
            .rate_states
            .get("sem.local")
            .map(|r| Arc::clone(&*r))
            .unwrap();

        let p1 = state.semaphore.clone().try_acquire_owned().ok();
        let p2 = state.semaphore.clone().try_acquire_owned().ok();
        let p3 = state.semaphore.clone().try_acquire_owned();

        assert!(p1.is_some(), "first permit acquired");
        assert!(p2.is_some(), "second permit acquired");
        assert!(p3.is_err(), "third permit denied (max_concurrent=2)");
        drop(p1);
        let p4 = state.semaphore.clone().try_acquire_owned();
        assert!(p4.is_ok(), "permit available after release");
    }
}
