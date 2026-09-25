use std::sync::{LazyLock, OnceLock};
use std::time::Instant;

use crate::service::AppService;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);
static BUILD_INFO: OnceLock<BuildInfo> = OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct BuildInfo {
    pub version: String,
    pub git_sha: String,
}

pub fn init(version: &str, git_sha: &str) {
    LazyLock::force(&START);
    let _ = BUILD_INFO.set(BuildInfo {
        version: version.to_string(),
        git_sha: git_sha.to_string(),
    });
}

fn build_info() -> BuildInfo {
    BUILD_INFO.get().cloned().unwrap_or_else(|| BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_sha: String::new(),
    })
}

pub fn current_version() -> String {
    build_info().version
}

pub fn uptime_secs() -> u64 {
    START.elapsed().as_secs()
}

#[derive(Debug, serde::Serialize)]
pub struct BrowserDiagnostics {
    /// Solver capability as last established, or "unknown" before the first
    /// probe. Never triggers one: diagnostics is a read-only surface.
    pub solver: String,
    pub calls_total: u64,
    pub restarts: u64,
    pub graceful_shutdowns: u64,
    pub forced_terminations: u64,
    pub solver_attempts: u64,
    pub solver_successes: u64,
    pub solver_failures: u64,
    pub max_memory_mb: u32,
    pub idle_timeout_s: u32,
}

#[derive(Debug, serde::Serialize)]
pub struct ExtensionStatus {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub loaded: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct DiagnosticsPayload {
    pub version: String,
    pub git_sha: String,
    pub uptime_secs: u64,
    pub db_size_bytes: u64,
    pub db_wal_size_bytes: u64,
    pub disk_free_data_bytes: Option<u64>,
    pub disk_free_library_bytes: Option<u64>,
    pub active_downloads: usize,
    pub jobs_running: usize,
    pub extensions: Vec<ExtensionStatus>,
    pub browser: BrowserDiagnostics,
    pub recent_error_count: u64,
    /// Subsystems running in a reduced state, worst first.
    pub degradations: Vec<super::degradations::Degradation>,
}

async fn file_len(path: std::path::PathBuf) -> u64 {
    tokio::fs::metadata(&path)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
}

impl AppService {
    /// Warns while the configured solver lacks the egress guard. An unreachable
    /// solver leaves the current state alone, since nothing new is known.
    pub async fn refresh_solver_egress_degradation(&self) {
        use crate::service::degradations::{Severity, ids::SOLVER_EGRESS_GUARD};
        match self.smart_client.solver_has_egress_guard().await {
            Some(false) => self.degradations.register(
                SOLVER_EGRESS_GUARD,
                Severity::Warn,
                "Browser solver",
                "The configured solver does not advertise kani.egress-guard/1. Browser sources \
                 are refused, and pages it loads while solving challenges can reach private, \
                 loopback and cloud-metadata addresses on its network.",
                "Run the flaresolverr-kani image as the solver, or keep the solver on a network \
                 with nothing else reachable.",
            ),
            Some(true) => self.degradations.clear(SOLVER_EGRESS_GUARD),
            None if !self.smart_client.solver_is_configured() => {
                self.degradations.clear(SOLVER_EGRESS_GUARD)
            }
            None => {}
        }
    }

    pub async fn get_diagnostics(&self) -> crate::error::Result<DiagnosticsPayload> {
        let info = build_info();

        let db_path = self.db_path.clone();
        let db_size_bytes = file_len(db_path.clone()).await;
        let db_wal_size_bytes = file_len(db_path.with_extension("db-wal")).await;

        let library_path = { self.settings.read().await.library_path.clone() };
        let data_path = db_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let disk_free_data_bytes =
            tokio::task::spawn_blocking(move || fs2::available_space(&data_path).ok())
                .await
                .unwrap_or(None);
        let lib_for_free = library_path.clone();
        let disk_free_library_bytes =
            tokio::task::spawn_blocking(move || fs2::available_space(&lib_for_free).ok())
                .await
                .unwrap_or(None);

        let active_downloads = self.downloader.snapshot().await.len();
        let jobs_running = self.job_manager.active_count();
        metrics::gauge!("kani_downloads_active").set(active_downloads as f64);
        metrics::gauge!("kani_jobs_running").set(jobs_running as f64);

        for circuit in self.smart_client.list_circuits() {
            if let Some(host) = circuit.get("host").and_then(|h| h.as_str()) {
                let is_open = circuit
                    .get("is_open")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                metrics::gauge!("kani_circuit_open", "host" => host.to_string()).set(if is_open {
                    1.0
                } else {
                    0.0
                });
            }
        }

        let rows = sqlx::query!("SELECT id, name, version, enabled FROM sources ORDER BY name")
            .fetch_all(&self.db_read)
            .await?;
        let extensions = rows
            .into_iter()
            .map(|r| ExtensionStatus {
                loaded: self.sources.contains_key(r.id),
                id: r.id,
                name: r.name,
                version: r.version,
                enabled: r.enabled,
            })
            .collect();

        let stats = kani_core::v8_process::browser_stats();
        let browser = BrowserDiagnostics {
            solver: self
                .smart_client
                .cached_solver_capability()
                .map_or("unknown", kani_core::http::SolverCapability::as_str)
                .to_string(),
            calls_total: stats.calls_total,
            restarts: stats.restarts,
            graceful_shutdowns: stats.graceful_shutdowns,
            forced_terminations: stats.forced_terminations,
            solver_attempts: stats.solver_attempts,
            solver_successes: stats.solver_successes,
            solver_failures: stats.solver_failures,
            max_memory_mb: stats.max_memory_mb,
            idle_timeout_s: stats.idle_timeout_s,
        };

        Ok(DiagnosticsPayload {
            version: info.version,
            git_sha: info.git_sha,
            uptime_secs: uptime_secs(),
            db_size_bytes,
            db_wal_size_bytes,
            disk_free_data_bytes,
            disk_free_library_bytes,
            active_downloads,
            jobs_running,
            extensions,
            browser,
            recent_error_count: 0,
            degradations: self.degradations.list(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn build_info_falls_back_to_crate_version_when_uninitialised() {
        let info = build_info();
        assert!(
            !info.version.is_empty(),
            "version must never be empty, even before init()"
        );
    }

    #[test]
    fn init_is_idempotent_and_uptime_is_monotonic() {
        init("1.2.3", "abc1234");
        let first = uptime_secs();
        init("9.9.9", "zzz");
        assert_eq!(
            build_info().version,
            "1.2.3",
            "init must not overwrite build info once set"
        );
        assert!(uptime_secs() >= first);
    }
}
