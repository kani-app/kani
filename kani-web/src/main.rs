const DEV_VENDOR_ASSETS: &[&str] = &[
    "js/vendor/preact.module.js",
    "js/vendor/preact-hooks.module.js",
    "js/vendor/htm.module.js",
    "js/vendor/signals-core.module.js",
    "js/vendor/signals.module.js",
    "js/vendor/compat.module.js",
    "js/vendor/debug.module.js",
    "js/vendor/devtools.module.js",
];

fn validate_dev_assets(assets: &kani_web::assets::Assets) -> Result<(), String> {
    let missing = DEV_VENDOR_ASSETS
        .iter()
        .filter(|path| assets.get(path).is_none())
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    Err(format!(
        "missing frontend vendor assets:\n  {}\n\nRun: cargo run -p kani-cli -- setup --vendors",
        missing.join("\n  ")
    ))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn inline_script_hashes(html: &[u8]) -> Result<Vec<String>, &'static str> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest as _, Sha256};

    let mut hashes = Vec::new();
    let mut remaining = html;
    while let Some(script_start) = find_subslice(remaining, b"<script") {
        remaining = &remaining[script_start..];
        let tag_end = find_subslice(remaining, b">").ok_or("unterminated script tag")?;
        let body_start = tag_end + 1;
        let body_end = find_subslice(&remaining[body_start..], b"</script>")
            .ok_or("unterminated inline script")?;
        let tag = &remaining[..tag_end];
        if !tag.windows(b" src=".len()).any(|window| window == b" src=") {
            let body = &remaining[body_start..body_start + body_end];
            hashes.push(format!(
                "'sha256-{}'",
                STANDARD.encode(Sha256::digest(body))
            ));
        }
        remaining = &remaining[body_start + body_end + b"</script>".len()..];
    }

    if hashes.is_empty() {
        return Err("no inline scripts found");
    }
    Ok(hashes)
}

fn script_src(assets: &kani_web::assets::Assets, index_name: &str) -> Result<String, String> {
    if !cfg!(debug_assertions) {
        return Ok("script-src 'self' 'wasm-unsafe-eval'".to_owned());
    }

    let index = assets
        .get(index_name)
        .ok_or_else(|| format!("frontend shell '{index_name}' is missing"))?;
    let hashes = inline_script_hashes(index.bytes.as_ref()).map_err(str::to_owned)?;
    Ok(format!(
        "script-src 'self' 'wasm-unsafe-eval' {}",
        hashes.join(" ")
    ))
}
/// Sets `Cache-Control` on all `/js/*` and `/css/*` responses.
///
/// Release builds get long-lived immutable caching — filenames are content-hashed.
/// Debug builds get `no-cache` so the browser always revalidates raw source files.
/// Without an explicit header the browser applies heuristic caching and can serve a
/// stale module after an edit, which surfaces as confusing parse errors.
/// Whether an asset's filename carries a content hash, and so can never change
/// meaning.
///
/// esbuild emits `name-HASH.js` with an uppercase base32 hash. Everything else
/// under `/js/` and `/css/` — the entry module, the stylesheet, the vendored
/// modules — keeps its path across rebuilds and must be revalidated.
pub(crate) fn is_content_hashed(path: &str) -> bool {
    let Some(file) = path.rsplit('/').next() else {
        return false;
    };
    let Some((stem, _ext)) = file.rsplit_once('.') else {
        return false;
    };
    let Some((_, hash)) = stem.rsplit_once('-') else {
        return false;
    };
    hash.len() >= 8
        && hash
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
}

async fn cache_control_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use rquest::header;

    // `immutable` is only true of a hashed name. Applied to a stable path it
    // tells the browser never to revalidate, which strands a client on a build
    // it can no longer escape without clearing site data.
    #[cfg(not(debug_assertions))]
    const IMMUTABLE: &str = "public, max-age=31536000, immutable";
    #[cfg(debug_assertions)]
    const IMMUTABLE: &str = "no-cache";
    const REVALIDATE: &str = "no-cache";

    let path = request.uri().path();
    let is_static = path.starts_with("/js/") || path.starts_with("/css/");
    // The worker is the only way to recover a client, so it can never be held.
    let is_worker = path == "/sw.js";
    let value = if is_content_hashed(path) {
        IMMUTABLE
    } else {
        REVALIDATE
    };

    let mut response = next.run(request).await;
    if is_static || is_worker {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static(value),
        );
    }
    response
}

/// The interval at which the limiter replenishes one request, for a given rate.
///
/// `GovernorConfigBuilder::per_second` is named for the unit of its argument,
/// not for what the argument means: it sets the replenishment *period*. Passing
/// a requests-per-second figure to it therefore inverts the limit — the larger
/// the intended rate, the slower the actual one.
/// Sustained requests per second, and the burst above it, for each bucket.
/// The tests below assert against these rather than repeating the numbers, so a
/// default cannot be lowered without the assertion that guards it moving too.
const API_RATE_RELEASE: u64 = 20;
const API_RATE_DEBUG: u64 = 200;
const API_BURST_RELEASE: u32 = 300;
const API_BURST_DEBUG: u32 = 4000;
const PROXY_RATE_RELEASE: u64 = 45;
const PROXY_RATE_DEBUG: u64 = 450;
const PROXY_BURST_RELEASE: u32 = 600;
const PROXY_BURST_DEBUG: u32 = 6000;

const _: () = {
    const OBSERVED_PEAK_PER_SECOND: u64 = 8;
    assert!(
        API_RATE_RELEASE > OBSERVED_PEAK_PER_SECOND,
        "release API rate does not clear measured browsing"
    );

    const OBSERVED_SESSION_CALLS: u32 = 148;
    assert!(
        API_BURST_RELEASE > OBSERVED_SESSION_CALLS,
        "release API burst cannot absorb a measured session"
    );

    // Debug exists to be looser than release. A larger rate is a shorter
    // period, so this also pins the direction of the conversion.
    assert!(
        API_RATE_DEBUG > API_RATE_RELEASE,
        "debug API rate must exceed release"
    );
    assert!(
        API_BURST_DEBUG > API_BURST_RELEASE,
        "debug API burst must exceed release"
    );
    assert!(
        PROXY_RATE_DEBUG > PROXY_RATE_RELEASE,
        "debug proxy rate must exceed release"
    );

    const OBSERVED_PROXY_PEAK: u64 = 18;

    // Both buckets use the same headroom rule so their release profiles cannot drift.
    const HEADROOM: u64 = 2;
    assert!(
        API_RATE_RELEASE >= OBSERVED_PEAK_PER_SECOND * HEADROOM,
        "release API rate leaves less headroom over measured browsing than the proxy does"
    );
    assert!(
        PROXY_RATE_RELEASE >= OBSERVED_PROXY_PEAK * HEADROOM,
        "release proxy rate leaves less headroom over a measured cover grid than the API does"
    );
};

fn replenish_period(requests_per_second: u64) -> std::time::Duration {
    const NANOS_PER_SEC: u64 = 1_000_000_000;
    std::time::Duration::from_nanos(NANOS_PER_SEC / requests_per_second.max(1))
}

/// Serves one asset tree under `/<prefix>/...`.
///
/// Replaces a `ServeDir` mount. The wildcard capture is the path *within* the
/// tree, so `/js/dist/app.js` resolves the asset `js/dist/app.js` whether that
/// comes from disk or from the binary.
fn asset_routes(prefix: &'static str, assets: &kani_web::assets::Assets) -> axum::Router {
    let a = assets.clone();
    axum::Router::new().route(
        &format!("/{prefix}/{{*path}}"),
        axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>,
                  headers: axum::http::HeaderMap| {
                let a = a.clone();
                async move { kani_web::assets::serve_prefixed(prefix, a, path, headers).await }
            },
        ),
    )
}

/// Serves a single named asset, for the SPA shell and the changelog.
fn named_asset(
    assets: &kani_web::assets::Assets,
    name: &'static str,
) -> impl Fn(
    axum::http::HeaderMap,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = axum::response::Response> + Send>>
+ Clone
+ Send
+ 'static {
    let a = assets.clone();
    move |headers: axum::http::HeaderMap| {
        let a = a.clone();
        Box::pin(async move { kani_web::assets::serve_named(name, a, headers).await })
    }
}

#[tokio::main]
async fn main() {
    use axum::Router;
    use axum::http::{HeaderValue, Method, StatusCode, header};
    use axum::response::IntoResponse;
    use axum_login::{
        AuthManagerLayerBuilder,
        tower_sessions::{SessionManagerLayer, cookie::SameSite},
    };
    use kani_web::{auth::AuthBackend, rest, state::AppState};
    use std::sync::Arc;
    use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
    use tower_http::{
        compression::CompressionLayer,
        cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer},
        set_header::SetResponseHeaderLayer,
        trace::TraceLayer,
    };
    use tower_sessions_sqlx_store::SqliteStore;

    let buf_cap: usize = std::env::var("KANI_LOG_BUFFER_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);

    let (ring_layer, log_handle) = kani_web::logging::RingBufferLayer::new(buf_cap);

    const DEFAULT_FILTER: &str =
        "info,axum_login=warn,tower_sessions=warn,tower_sessions_core=warn,sqlx=warn";

    let make_filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_FILTER))
    };

    use tracing_subscriber::prelude::*;

    let json_logs = std::env::var("KANI_LOG_FORMAT").is_ok_and(|v| v.eq_ignore_ascii_case("json"));

    let fmt_layer = if json_logs {
        tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .with_filter(make_filter())
            .boxed()
    } else {
        tracing_subscriber::fmt::layer()
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .with_filter(make_filter())
            .boxed()
    };

    let registry = tracing_subscriber::registry()
        .with(ring_layer.with_filter(make_filter()))
        .with(fmt_layer);

    registry.init();

    kani_app::service::diagnostics::init(env!("CARGO_PKG_VERSION"), env!("GIT_SHA"));

    tracing::info!("Starting Kani Web Server");

    // Resolve the data directory. In Docker the working directory is /data, so the
    // default (CWD) is correct without any configuration. Native installs can
    // override with KANI_DATA_DIR.
    let data_dir: std::path::PathBuf = std::env::var("KANI_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir().expect("Cannot determine current working directory")
        });

    let state = AppState::new(log_handle, data_dir)
        .await
        .expect("Failed to initialise AppState");

    let session_store = SqliteStore::new(state.db.clone());
    session_store
        .migrate()
        .await
        .expect("Failed to migrate session store");

    let secure_cookies = std::env::var("KANI_SECURE_COOKIES")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    // Session lifetime — defaults to 30 days of inactivity. Read from the
    // `session_timeout_secs` setting (seeded from KANI_SESSION_TIMEOUT_SECONDS on
    // first boot); changes apply on restart.
    let session_timeout_secs: i64 = state.settings.read().await.session_timeout_secs;

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(secure_cookies)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_expiry(axum_login::tower_sessions::Expiry::OnInactivity(
            time::Duration::seconds(session_timeout_secs),
        ));

    let auth_backend = AuthBackend::new(state.db.clone());
    let auth_layer = AuthManagerLayerBuilder::new(auth_backend.clone(), session_layer).build();

    if let Err(e) = announce_first_run(&auth_backend).await {
        tracing::error!("Failed to check for a first-run instance: {}", e);
    }

    kani_web::HTTP_LOGGING_ENABLED.store(
        state.get_settings().await.http_request_logging,
        std::sync::atomic::Ordering::Relaxed,
    );

    kani_web::SOURCE_INSTALL_ALLOWED.store(
        std::env::var("KANI_SOURCE_INSTALL_ALLOWED")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true),
        std::sync::atomic::Ordering::Relaxed,
    );

    {
        let official_url = std::env::var("KANI_OFFICIAL_REPO_URL").unwrap_or_default();
        let official_key = std::env::var("KANI_OFFICIAL_REPO_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| kani_web::repo_keys::OFFICIAL_REPO_KEY.to_string());
        if !official_url.is_empty() && !official_key.is_empty() {
            let bootstrap_state = state.clone();
            tokio::spawn(async move {
                if let Err(e) = bootstrap_state
                    .bootstrap_official_repo(&official_url, &official_key)
                    .await
                {
                    tracing::warn!("Official repo bootstrap failed: {e}");
                }
            });
        }
    }

    state.spawn_cover_retry();
    state.spawn_credential_refresh();
    state.spawn_webhook_listener();
    state.spawn_login_attempt_prune();
    state.spawn_cache_prune();
    {
        let solver_state = state.clone();
        tokio::spawn(async move { solver_state.refresh_solver_egress_degradation().await });
    }
    state.spawn_progress_flush();

    {
        let backfill_state = state.clone();
        tokio::spawn(async move {
            backfill_state.submit_manifest_backfill_if_needed().await;

            // After the backfill, so a startup scrub judges the paths and
            // hashes the backfill just wrote rather than the gaps it filled.
            if backfill_state.get_settings().await.scrub_on_startup {
                let job = kani_app::jobs::scrub::ScrubJob::new(
                    kani_app::service::integrity::ScrubDepth::Quick,
                    false,
                );
                if let Err(e) = backfill_state.service.job_manager.submit(job).await {
                    tracing::warn!("Failed to submit startup integrity scrub: {e}");
                }
            }
        });
    }

    if let Err(e) = kani_app::jobs::recurring::ensure_recurring_rows(&state.db).await {
        tracing::warn!("Failed to initialise recurring job rows: {e}");
    }
    kani_app::jobs::recurring::spawn_recurring_scheduler(&state);

    if state.service.pending_import_link_count().await > 0
        && let Err(e) = state.service.queue_import_resolve(None).await
    {
        tracing::warn!("Failed to submit startup import relink sweep: {e}");
    }

    let dev_build = cfg!(debug_assertions);
    let env_u64 = |name: &str, default: u64| -> u64 {
        std::env::var(name)
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(default)
    };
    let env_u32 = |name: &str, default: u32| -> u32 {
        std::env::var(name)
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(default)
    };

    let api_rate_per_second = env_u64(
        "KANI_API_RATE_PER_SECOND",
        if dev_build {
            API_RATE_DEBUG
        } else {
            API_RATE_RELEASE
        },
    );
    let api_burst_size = env_u32(
        "KANI_API_BURST_SIZE",
        if dev_build {
            API_BURST_DEBUG
        } else {
            API_BURST_RELEASE
        },
    );
    let proxy_rate_per_second = env_u64(
        "KANI_PROXY_RATE_PER_SECOND",
        if dev_build {
            PROXY_RATE_DEBUG
        } else {
            PROXY_RATE_RELEASE
        },
    );
    let proxy_burst_size = env_u32(
        "KANI_PROXY_BURST_SIZE",
        if dev_build {
            PROXY_BURST_DEBUG
        } else {
            PROXY_BURST_RELEASE
        },
    );

    let api_period = replenish_period(api_rate_per_second);
    let proxy_period = replenish_period(proxy_rate_per_second);

    if dev_build {
        tracing::info!(
            api_rate_per_second,
            api_burst_size,
            "Debug build: rate limits relaxed for local development"
        );
    }

    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .period(api_period)
            .burst_size(api_burst_size)
            // Bucket bearer traffic per token, so a busy integration cannot
            // spend its owner's browsing budget.
            .key_extractor(kani_web::rate_limit_key::TokenOrPeerIp::new(
                state.trusted_proxies.clone(),
            ))
            // Emit x-ratelimit-* and retry-after: a client that cannot see its
            // budget can only retry blindly, which makes congestion worse.
            .use_headers()
            .finish()
            .expect("api rate/burst values are valid"),
    );
    let proxy_governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .period(proxy_period)
            .burst_size(proxy_burst_size)
            .finish()
            .expect("proxy rate/burst values are valid"),
    );

    let cors_layer = {
        let allow_origin = std::env::var("KANI_CORS_ORIGIN")
            .map(|origin| {
                origin
                    .parse::<HeaderValue>()
                    .map(AllowOrigin::exact)
                    .unwrap_or_else(|_| AllowOrigin::mirror_request())
            })
            .unwrap_or_else(|_| AllowOrigin::mirror_request());

        CorsLayer::new()
            .allow_origin(allow_origin)
            .allow_methods(AllowMethods::list([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ]))
            .allow_headers(AllowHeaders::mirror_request())
            .allow_credentials(true)
    };

    let rest_router = rest::routes(state.clone());
    let proxy_router = rest::image_proxy_route(state.clone());
    let opds_router = kani_web::opds::routes(state.clone());
    let health_router = axum::Router::new()
        .route("/health", axum::routing::get(rest::health))
        .route("/healthz", axum::routing::get(rest::health))
        .route("/ready", axum::routing::get(rest::ready))
        .route("/readyz", axum::routing::get(rest::ready))
        .with_state(state.clone());
    let (prometheus_layer, _) = kani_web::metrics::prometheus();
    kani_web::metrics::describe();
    let metrics_router = kani_web::metrics::router(state.clone());

    {
        let db = state.db.clone();
        let mut rx = state.downloader.subscribe();
        let listener_state = state.clone();
        let token = state.shutdown_token.clone();
        tokio::spawn(async move {
            loop {
                let event = tokio::select! {
                    _ = token.cancelled() => {
                        tracing::info!("Download listener shutting down");
                        break;
                    }
                    result = rx.recv() => result,
                };
                match event {
                    Ok(event) => match event {
                        kani_shared::DownloadProgressEvent::ChapterCompleted {
                            chapter_id,
                            successful_pages,
                            ..
                        } => {
                            let pc = successful_pages as i64;
                            let _ = sqlx::query!(
                                "UPDATE chapters SET page_count = ? WHERE id = ?",
                                pc,
                                chapter_id
                            )
                            .execute(&db)
                            .await;
                            struct ChapterInfo {
                                manga_id: i64,
                                manga_name: String,
                                volume: Option<i64>,
                                chapter_number: f64,
                                name: Option<String>,
                            }
                            if let Ok(Some(info)) = sqlx::query_as!(
                                ChapterInfo,
                                "SELECT c.manga_id, m.name AS manga_name, \
                                 c.volume, c.chapter_number, c.name \
                                 FROM chapters c JOIN manga m ON c.manga_id = m.id \
                                 WHERE c.id = ?",
                                chapter_id
                            )
                            .fetch_optional(&db)
                            .await
                            {
                                let chapter_name = kani_app::chapter_name(
                                    info.volume,
                                    info.chapter_number,
                                    info.name,
                                );
                                listener_state
                                    .fire_webhooks(
                                        kani_app::service::webhooks::WebhookPayload::ChapterDownloaded {
                                            chapter_id: kani_app::ids::ChapterId(chapter_id),
                                            manga_id: kani_app::ids::MangaId(info.manga_id),
                                            manga_name: info.manga_name,
                                            chapter_name,
                                        },
                                    )
                                    .await;
                            }
                        }
                        kani_shared::DownloadProgressEvent::ChapterFailed { error, .. } => {
                            tracing::error!("Chapter download failed: {}", error);
                        }
                        _ => {}
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::error!("Download progress channel closed.");
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            "Download progress listener lagged by {} events! Reconciling...",
                            skipped
                        );
                        let sweep_state = listener_state.clone();
                        tokio::spawn(async move {
                            let records = sqlx::query!("SELECT c.id, c.volume, c.chapter_number, c.name, m.id as manga_id, m.name as manga_name FROM chapters c JOIN manga m ON c.manga_id = m.id WHERE c.download_status = 1")
                                .fetch_all(&sweep_state.db)
                                .await
                                .unwrap_or_default();

                            let library_path =
                                sweep_state.settings.read().await.library_path.clone();
                            for record in records {
                                let safe_manga_name_base =
                                    kani_core::utilities::sanitize_filename(&record.manga_name);
                                let safe_manga_name =
                                    format!("{} - {}", safe_manga_name_base, record.manga_id);
                                let manga_path = library_path.join(safe_manga_name);

                                let chapter_name = kani_web::state::chapter_name(
                                    record.volume,
                                    record.chapter_number,
                                    record.name,
                                );
                                let safe_chapter_name =
                                    kani_core::utilities::sanitize_filename(&chapter_name);
                                let file_path =
                                    manga_path.join(format!("{}.cbz", safe_chapter_name));

                                if tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
                                    if let Err(e) = sqlx::query!(
                                        "UPDATE chapters SET download_status = ?, downloaded_at = COALESCE(downloaded_at, CURRENT_TIMESTAMP) WHERE id = ?",
                                        kani_shared::types::DownloadStatus::Complete,
                                        record.id
                                    )
                                    .execute(&sweep_state.db)
                                    .await
                                    {
                                        tracing::warn!(
                                            "Reconcile: failed to set download_status=2 for chapter {}: {}",
                                            record.id,
                                            e
                                        );
                                    }
                                } else if let Err(e) = sqlx::query!(
                                    "UPDATE chapters SET download_status = ? WHERE id = ?",
                                    kani_shared::types::DownloadStatus::Pending,
                                    record.id
                                )
                                .execute(&sweep_state.db)
                                .await
                                {
                                    tracing::warn!(
                                        "Reconcile: failed to reset download_status for chapter {}: {}",
                                        record.id,
                                        e
                                    );
                                }
                            }
                        });
                    }
                }
            }
        });
    }

    // Resolved once: KANI_STATIC_DIR, else the copy embedded in a release
    // binary, else ./static on a debug build. `Assets::resolve` logs which,
    // because "every page 404s" is otherwise hard to attribute.
    let assets = kani_web::assets::Assets::resolve();

    // Debug serves raw modules through an import map; release serves the bundled entry point.
    let index_name = if cfg!(debug_assertions) {
        "index.html"
    } else {
        "index.prod.html"
    };

    if cfg!(debug_assertions) {
        validate_dev_assets(&assets).unwrap_or_else(|error| panic!("{error}"));
    }
    let script_src = script_src(&assets, index_name)
        .unwrap_or_else(|error| panic!("invalid frontend shell: {error}"));

    let app = Router::new()
        .route(
            "/manifest.webmanifest",
            axum::routing::get({
                let a = assets.clone();
                move || {
                    let a = a.clone();
                    async move {
                        // Explicit content type: the manifest needs
                        // application/manifest+json, which mime_guess will not infer.
                        match a.get("manifest.webmanifest") {
                            Some(asset) => (
                                [(
                                    header::CONTENT_TYPE,
                                    "application/manifest+json; charset=utf-8",
                                )],
                                asset.bytes.into_owned(),
                            )
                                .into_response(),
                            None => StatusCode::NOT_FOUND.into_response(),
                        }
                    }
                }
            }),
        )
        .route(
            "/sw.js",
            axum::routing::get({
                let a = assets.clone();
                move || {
                    let a = a.clone();
                    async move {
                        // Service-Worker-Allowed lets a worker served from /sw.js
                        // claim the whole origin; without it the scope is /.
                        match a.get("sw.js") {
                            Some(asset) => {
                                let mut h = axum::http::HeaderMap::new();
                                h.insert(
                                    header::CONTENT_TYPE,
                                    header::HeaderValue::from_static(
                                        "application/javascript; charset=utf-8",
                                    ),
                                );
                                h.insert(
                                    header::HeaderName::from_static("service-worker-allowed"),
                                    header::HeaderValue::from_static("/"),
                                );
                                (StatusCode::OK, h, asset.bytes.into_owned()).into_response()
                            }
                            None => StatusCode::NOT_FOUND.into_response(),
                        }
                    }
                }
            }),
        )
        .merge(asset_routes("icons", &assets))
        // OPDS catalog — auth is handled per-handler (supports Basic auth)
        .nest("/opds", opds_router)
        .merge(
            Router::new()
                .nest("/rest", rest_router)
                .layer(GovernorLayer {
                    config: governor_conf,
                }),
        )
        .merge(
            Router::new()
                .nest("/rest", proxy_router)
                .layer(GovernorLayer {
                    config: proxy_governor_conf,
                }),
        )
        .merge(health_router)
        .merge(metrics_router)
        .merge(asset_routes("js", &assets))
        .merge(asset_routes("css", &assets))
        .merge(asset_routes("locales", &assets))
        .merge(asset_routes("fonts", &assets))
        .route(
            "/changelog.md",
            axum::routing::get(named_asset(&assets, "changelog.md")),
        )
        // Anything unmatched is a client-side route, so the SPA shell answers.
        .fallback(named_asset(&assets, index_name))
        .layer(axum::middleware::from_fn(kani_web::auth::auth_guard))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            kani_web::session_touch::session_touch_middleware,
        ))
        .layer(auth_layer)
        // Outside the session layer, so the response it inspects already carries
        // any rotated session cookie.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            kani_web::csrf::csrf_middleware,
        ))
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::try_from(format!(
                "default-src 'self'; \
                 img-src 'self' data: blob:; \
                 style-src 'self' 'unsafe-inline'; \
                 {script_src}; \
                 object-src 'none'; \
                 base-uri 'self'; \
                 form-action 'self'; \
                 frame-ancestors 'none'"
            ))
            .expect("CSP header value is statically valid"),
        ))
        // HSTS: only when KANI_SECURE_COOKIES=true, meaning TLS is terminated upstream.
        .layer(tower::util::option_layer(if secure_cookies {
            Some(SetResponseHeaderLayer::overriding(
                axum::http::HeaderName::from_static("strict-transport-security"),
                HeaderValue::from_static("max-age=31536000; includeSubDomains"),
            ))
        } else {
            None
        }))
        .layer(cors_layer)
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                if kani_web::HTTP_LOGGING_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
                    let request_id = request
                        .extensions()
                        .get::<tower_http::request_id::RequestId>()
                        .and_then(|id| id.header_value().to_str().ok())
                        .unwrap_or_default();
                    tracing::info_span!(
                        "http",
                        method = %request.method(),
                        uri = %request.uri(),
                        status = tracing::field::Empty,
                        request_id = %request_id,
                    )
                } else {
                    tracing::Span::none()
                }
            }),
        );

    #[cfg(debug_assertions)]
    let app = {
        use utoipa::OpenApi;
        app.merge(utoipa_swagger_ui::SwaggerUi::new("/api-docs").url(
            "/api-docs/openapi.json",
            kani_web::openapi::ApiDoc::openapi(),
        ))
    };

    // Cache-Control for static assets: immutable in release, no-cache in debug.
    let app = app
        .layer(axum::middleware::from_fn(cache_control_middleware))
        .layer(prometheus_layer.clone())
        .layer(tower_http::request_id::PropagateRequestIdLayer::x_request_id())
        .layer(tower_http::request_id::SetRequestIdLayer::x_request_id(
            kani_web::middleware::trace_id::UuidRequestId,
        ));

    let bind_addr = std::env::var("KANI_BIND").unwrap_or_else(|_| "0.0.0.0:8242".into());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|_| panic!("Failed to bind {bind_addr}"));

    let shutdown_token = state.shutdown_token.clone();

    tracing::info!("Server listening on http://{}", bind_addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl-C received, shutting down...");
            }
            _ = shutdown_token.cancelled() => {
                tracing::info!("Shutdown token cancelled, shutting down...");
            }
        }
        tracing::info!("Stopping background tasks...");
        shutdown_token.cancel();
        let drain_secs = std::env::var("KANI_JOB_SHUTDOWN_TIMEOUT_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30);
        state
            .job_manager
            .drain(std::time::Duration::from_secs(drain_secs))
            .await;
        state.sources.retire_all("server-shutdown").await;
        state.db.close().await;
        let exit_code = if state
            .restart_requested
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            tracing::info!("Restart requested — exiting with code 42.");
            42
        } else {
            tracing::info!("Shutdown complete, exiting.");
            0
        };
        std::process::exit(exit_code);
    })
    .await
    .unwrap_or_else(|e| panic!("Server error: {e}"));
}

/// A fresh instance has no accounts and no generated password: the first person
/// to reach it over the local network creates the administrator through the
/// in-app setup screen (`POST /rest/auth/setup`), which closes the moment that
/// account exists.
async fn announce_first_run(
    backend: &kani_web::auth::AuthBackend,
) -> Result<(), kani_web::error::AppError> {
    if backend.user_count().await? == 0 {
        tracing::info!(
            "No accounts yet — open Kani in a browser on this machine or your local \
             network to create the administrator. Setup closes as soon as that \
             account exists. Set KANI_ALLOW_REMOTE_SETUP=true if you must do it \
             over the internet."
        );
    }
    Ok(())
}

#[cfg(test)]
mod rate_limit_config_tests {
    use super::replenish_period;
    use std::time::Duration;

    #[test]
    fn missing_development_vendors_report_the_recovery_command() {
        let dir = tempfile::tempdir().expect("temporary asset directory should exist");
        let error =
            super::validate_dev_assets(&kani_web::assets::Assets::Disk(dir.path().to_path_buf()))
                .expect_err("an empty asset directory is invalid for debug startup");

        assert!(error.contains("js/vendor/preact.module.js"));
        assert!(error.contains("cargo run -p kani-cli -- setup --vendors"));
    }
    #[test]
    fn inline_script_hashes_ignore_external_scripts() {
        let hashes = super::inline_script_hashes(
            br#"<script>window.boot()</script><script type="module" src="/js/app.js"></script>"#,
        )
        .expect("well-formed scripts should hash");

        assert_eq!(hashes.len(), 1);
        assert_eq!(
            hashes[0],
            "'sha256-7V6Igl78kAwCDPrKA2yl1sq4zWHW8UjPYq8fNJg+cQE='"
        );
    }

    #[test]
    fn inline_script_hashes_preserve_line_endings() {
        let lf = super::inline_script_hashes(b"<script>one\ntwo</script>")
            .expect("LF script should hash");
        let crlf = super::inline_script_hashes(b"<script>one\r\ntwo</script>")
            .expect("CRLF script should hash");

        assert_ne!(lf, crlf);
    }

    #[test]
    fn inline_script_hashes_reject_malformed_shells() {
        assert_eq!(
            super::inline_script_hashes(b"<main></main>"),
            Err("no inline scripts found")
        );
        assert_eq!(
            super::inline_script_hashes(b"<script>window.boot()"),
            Err("unterminated inline script")
        );
    }
    #[test]
    fn a_rate_becomes_its_reciprocal() {
        assert_eq!(replenish_period(1), Duration::from_secs(1));
        assert_eq!(replenish_period(5), Duration::from_millis(200));
        assert_eq!(replenish_period(50), Duration::from_millis(20));
        assert_eq!(replenish_period(500), Duration::from_millis(2));
    }

    #[test]
    fn a_higher_rate_is_never_a_stricter_limit() {
        let mut previous = replenish_period(1);
        for rate in [2, 5, 30, 50, 300, 500, 20_000] {
            let period = replenish_period(rate);
            assert!(
                period < previous,
                "rate {rate} produced period {period:?}, not shorter than {previous:?}"
            );
            previous = period;
        }
    }

    #[test]
    fn zero_does_not_divide_by_zero() {
        assert_eq!(replenish_period(0), Duration::from_secs(1));
    }

    #[test]
    fn the_period_helper_is_used_by_both_buckets() {
        for rate in [super::API_RATE_RELEASE, super::PROXY_RATE_RELEASE] {
            assert_eq!(
                replenish_period(rate),
                Duration::from_nanos(1_000_000_000 / rate),
                "the period must be the reciprocal of the rate"
            );
        }
        assert!(
            replenish_period(super::PROXY_RATE_RELEASE) < replenish_period(super::API_RATE_RELEASE)
        );
    }

    #[test]
    fn only_hashed_filenames_are_immutable() {
        // esbuild's output: the hash is what makes the name safe to pin.
        assert!(super::is_content_hashed("/js/dist/chunk-5M4XVRNJ.js"));
        assert!(super::is_content_hashed("/js/dist/settings-QDVWL77B.js"));

        // Rebuilt in place on every frontend change. Pinning these is what
        // stranded clients on a build they could not escape.
        assert!(!super::is_content_hashed("/js/dist/app.js"));
        assert!(!super::is_content_hashed("/css/main.css"));
        assert!(!super::is_content_hashed("/js/vendor/preact.module.js"));
        assert!(!super::is_content_hashed("/js/app.js"));

        // A hyphen alone is not a hash.
        assert!(!super::is_content_hashed("/js/dist/some-widget.js"));
        assert!(!super::is_content_hashed("/js/dist/a-BC12.js"));
    }
}
