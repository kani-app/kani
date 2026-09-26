//! Lifecycle and newline-delimited protocol for the shared Node.js browser worker.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

static V8_DEBUG_LOGGING: AtomicBool = AtomicBool::new(false);

pub fn set_v8_debug_logging(enabled: bool) {
    V8_DEBUG_LOGGING.store(enabled, Ordering::Relaxed);
}

/// Whether the operator has turned on script-engine debug logging in settings.
/// Read by the other half of the browser-capture path — the solver client in
/// [`crate::http`] — so one toggle covers both engines rather than each growing
/// its own switch.
pub fn v8_debug_logging_enabled() -> bool {
    V8_DEBUG_LOGGING.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug)]
pub struct V8Config {
    pub max_memory_mb: u32,
    pub idle_timeout_s: u32,
}

impl Default for V8Config {
    fn default() -> Self {
        Self {
            max_memory_mb: 512,
            idle_timeout_s: 300,
        }
    }
}

static V8_CONFIG: RwLock<V8Config> = RwLock::new(V8Config {
    max_memory_mb: 512,
    idle_timeout_s: 300,
});

pub fn set_v8_config(cfg: V8Config) {
    if let Ok(mut guard) = V8_CONFIG.write() {
        *guard = cfg;
    }
}

pub(crate) fn v8_config() -> V8Config {
    V8_CONFIG.read().map(|g| *g).unwrap_or_default()
}

static V8_CALLS_TOTAL: AtomicU64 = AtomicU64::new(0);
static V8_RESTARTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static V8_GRACEFUL_SHUTDOWNS_TOTAL: AtomicU64 = AtomicU64::new(0);
static V8_FORCED_TERMINATIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static BROWSER_SOLVER_ATTEMPTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static BROWSER_SOLVER_SUCCESSES_TOTAL: AtomicU64 = AtomicU64::new(0);
static BROWSER_SOLVER_FAILURES_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Snapshot of browser-runtime counters plus the active resource caps, for the
/// diagnostics surface. Lives in kani-shared so kani-web can serialize it.
pub fn browser_stats() -> kani_shared::types::BrowserStats {
    let cfg = v8_config();
    kani_shared::types::BrowserStats {
        calls_total: V8_CALLS_TOTAL.load(Ordering::Relaxed),
        restarts: V8_RESTARTS_TOTAL.load(Ordering::Relaxed),
        graceful_shutdowns: V8_GRACEFUL_SHUTDOWNS_TOTAL.load(Ordering::Relaxed),
        forced_terminations: V8_FORCED_TERMINATIONS_TOTAL.load(Ordering::Relaxed),
        solver_attempts: BROWSER_SOLVER_ATTEMPTS_TOTAL.load(Ordering::Relaxed),
        solver_successes: BROWSER_SOLVER_SUCCESSES_TOTAL.load(Ordering::Relaxed),
        solver_failures: BROWSER_SOLVER_FAILURES_TOTAL.load(Ordering::Relaxed),
        max_memory_mb: cfg.max_memory_mb,
        idle_timeout_s: cfg.idle_timeout_s,
    }
}

pub(crate) fn record_browser_solver_result(success: bool) {
    BROWSER_SOLVER_ATTEMPTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    if success {
        BROWSER_SOLVER_SUCCESSES_TOTAL.fetch_add(1, Ordering::Relaxed);
    } else {
        BROWSER_SOLVER_FAILURES_TOTAL.fetch_add(1, Ordering::Relaxed);
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static V8_SHIM_PATH: OnceLock<Result<std::path::PathBuf, String>> = OnceLock::new();

/// Timeout for V8 IPC requests that don't carry their own caller-supplied
/// timeout (context create/eval/exists/drop).
const V8_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

const V8_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

pub type V8ProcessHandle = Arc<Mutex<V8Slot>>;

#[derive(Debug)]
pub enum V8Slot {
    Empty,
    Running(Box<V8Process>, Instant),
    Retired,
}

/// Constructs a fresh, empty V8 process handle; the process itself is
/// spawned lazily on first use.
pub fn new_handle() -> V8ProcessHandle {
    Arc::new(Mutex::new(V8Slot::Empty))
}

#[derive(Debug)]
pub struct V8Process {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

#[derive(Debug)]
enum V8RequestError {
    Action(String),
    Transport(String),
}

impl V8RequestError {
    fn into_message(self) -> String {
        match self {
            Self::Action(message) => message,
            Self::Transport(message) => message,
        }
    }
}

impl V8Process {
    async fn spawn() -> Result<Self, String> {
        let shim_path = V8_SHIM_PATH.get_or_init(|| {
            let path = std::env::temp_dir().join(format!("kani_v8_shim_{}.js", std::process::id()));
            std::fs::write(&path, include_str!("v8_shim.js"))
                .map(|()| path)
                .map_err(|e| format!("Failed to write v8 shim: {e}"))
        });
        let shim_path = shim_path.as_ref().map_err(|error| error.clone())?;

        let cfg = v8_config();
        let mut command = Command::new("node");
        command
            .arg(format!("--max-old-space-size={}", cfg.max_memory_mb))
            .arg(shim_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|e| format!("Failed to spawn node: {e}. Is Node.js installed?"))?;

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout_raw = child.stdout.take().ok_or("no stdout")?;
        let mut stdout = BufReader::new(stdout_raw);

        let mut line = String::new();
        tokio::time::timeout(V8_REQUEST_TIMEOUT, stdout.read_line(&mut line))
            .await
            .map_err(|_| "V8 shim startup timed out".to_string())?
            .map_err(|e| format!("V8 shim startup error: {e}"))?;
        if !line.contains("ready") {
            return Err(format!("V8 shim did not signal ready: {line}"));
        }
        if V8_DEBUG_LOGGING.load(Ordering::Relaxed) {
            eprintln!("[v8] worker spawned pid={:?}", child.id());
        }

        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    async fn request(
        &mut self,
        action: &str,
        name: &str,
        script: &str,
    ) -> Result<String, V8RequestError> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let msg = serde_json::json!({ "id": id, "action": action, "name": name, "script": script });
        let mut line = serde_json::to_string(&msg)
            .map_err(|e| V8RequestError::Transport(format!("V8 request encode error: {e}")))?;
        line.push('\n');

        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| V8RequestError::Transport(format!("V8 write error: {e}")))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| V8RequestError::Transport(format!("V8 flush error: {e}")))?;

        let mut resp = String::new();
        self.stdout
            .read_line(&mut resp)
            .await
            .map_err(|e| V8RequestError::Transport(format!("V8 read error: {e}")))?;

        if resp.is_empty() {
            return Err(V8RequestError::Transport(
                "V8 worker closed its output stream".to_string(),
            ));
        }

        let val: serde_json::Value = serde_json::from_str(resp.trim())
            .map_err(|e| V8RequestError::Transport(format!("V8 bad response: {e}")))?;

        if val.get("id").and_then(serde_json::Value::as_u64) != Some(id) {
            return Err(V8RequestError::Transport(
                "V8 response id did not match the request".to_string(),
            ));
        }

        if let Some(metrics) = val.get("metrics") {
            V8_GRACEFUL_SHUTDOWNS_TOTAL.fetch_add(
                metrics
                    .get("gracefulShutdowns")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                Ordering::Relaxed,
            );
            V8_FORCED_TERMINATIONS_TOTAL.fetch_add(
                metrics
                    .get("forcedTerminations")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                Ordering::Relaxed,
            );
        }

        match val.get("ok").and_then(serde_json::Value::as_bool) {
            Some(true) => Ok(val["value"].as_str().unwrap_or("").to_string()),
            Some(false) => {
                let error = val.get("error");
                let message = error
                    .and_then(|value| value.get("message"))
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| error.and_then(serde_json::Value::as_str))
                    .unwrap_or("unknown V8 error")
                    .to_string();
                Err(V8RequestError::Action(message))
            }
            None => Err(V8RequestError::Transport(
                "V8 response did not contain a boolean 'ok' field".to_string(),
            )),
        }
    }

    async fn force_terminate(&mut self, reason: &str) {
        if V8_DEBUG_LOGGING.load(Ordering::Relaxed) {
            eprintln!(
                "[v8] force terminating worker pid={:?} reason={reason}",
                self.child.id()
            );
        }
        V8_FORCED_TERMINATIONS_TOTAL.fetch_add(1, Ordering::Relaxed);

        #[cfg(windows)]
        if let Some(pid) = self.child.id() {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
        }

        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            let _ = Command::new("kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
        }

        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    async fn shutdown(self, reason: &str) {
        self.shutdown_with_timeout(reason, V8_SHUTDOWN_TIMEOUT)
            .await;
    }

    async fn shutdown_with_timeout(mut self, reason: &str, timeout: Duration) -> bool {
        if V8_DEBUG_LOGGING.load(Ordering::Relaxed) {
            eprintln!(
                "[v8] shutting down worker pid={:?} reason={reason}",
                self.child.id()
            );
        }
        if timeout.is_zero() {
            self.force_terminate(reason).await;
            return false;
        }
        let graceful = tokio::time::timeout(timeout, async {
            self.request("shutdown", reason, "")
                .await
                .map_err(V8RequestError::into_message)?;
            self.child
                .wait()
                .await
                .map_err(|e| format!("V8 worker wait failed: {e}"))?;
            Ok::<(), String>(())
        })
        .await;

        if matches!(graceful, Ok(Ok(()))) {
            V8_GRACEFUL_SHUTDOWNS_TOTAL.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            self.force_terminate(reason).await;
            false
        }
    }
}

async fn ensure_running(handle: &V8ProcessHandle) -> Result<(), String> {
    let mut guard = handle.lock().await;
    match &mut *guard {
        V8Slot::Running(_, last_used) => {
            *last_used = Instant::now();
            return Ok(());
        }
        V8Slot::Retired => return Err("V8 worker owner has been retired".to_string()),
        V8Slot::Empty => {}
    }
    *guard = V8Slot::Running(Box::new(V8Process::spawn().await?), Instant::now());
    Ok(())
}

/// Gracefully closes and clears the subprocess if it has been idle for at least `idle_for`.
/// Returns `true` when a process was reaped. Safe to call on a never-spawned or
/// already-cleared handle (returns `false`).
pub async fn reap_if_idle(handle: &V8ProcessHandle, idle_for: Duration) -> bool {
    let mut guard = handle.lock().await;
    let idle = match &*guard {
        V8Slot::Running(_, last_used) => last_used.elapsed() >= idle_for,
        V8Slot::Empty | V8Slot::Retired => return false,
    };
    if idle {
        if let V8Slot::Running(proc, _) = std::mem::replace(&mut *guard, V8Slot::Empty) {
            proc.shutdown("idle-reap").await;
        }
        true
    } else {
        false
    }
}

pub async fn shutdown(handle: &V8ProcessHandle, reason: &str) -> bool {
    let mut guard = handle.lock().await;
    match std::mem::replace(&mut *guard, V8Slot::Empty) {
        V8Slot::Running(proc, _) => {
            proc.shutdown(reason).await;
            true
        }
        V8Slot::Retired => {
            *guard = V8Slot::Retired;
            false
        }
        V8Slot::Empty => false,
    }
}

pub async fn retire(handle: &V8ProcessHandle, reason: &str) -> bool {
    let mut guard = handle.lock().await;
    match std::mem::replace(&mut *guard, V8Slot::Retired) {
        V8Slot::Running(proc, _) => {
            proc.shutdown(reason).await;
            true
        }
        V8Slot::Empty | V8Slot::Retired => false,
    }
}

/// Sends one request to the live V8 process, bounded by `timeout`.
///
/// Timing out drops the in-flight write/read futures immediately (tokio's
/// async I/O cancellation is cooperative and safe, unlike a blocking-thread
/// approach which cannot be interrupted once stuck) and additionally kills
/// the subprocess so a genuinely CPU-bound guest script doesn't linger. On
/// Action errors keep the worker alive because the newline protocol remains in
/// sync. Transport failures and timeouts retire it before the next request.
async fn with_process_detailed(
    handle: &V8ProcessHandle,
    timeout: Duration,
    action: &str,
    name: &str,
    script: &str,
) -> Result<String, V8RequestError> {
    ensure_running(handle)
        .await
        .map_err(V8RequestError::Transport)?;

    let outcome = tokio::time::timeout(timeout, async {
        let mut guard = handle.lock().await;
        let V8Slot::Running(proc, last_used) = &mut *guard else {
            return Err(V8RequestError::Transport(
                "V8 process not running".to_string(),
            ));
        };
        *last_used = Instant::now();
        proc.request(action, name, script).await
    })
    .await;

    match outcome {
        Ok(Ok(v)) => {
            V8_CALLS_TOTAL.fetch_add(1, Ordering::Relaxed);
            Ok(v)
        }
        Ok(Err(V8RequestError::Action(error))) => Err(V8RequestError::Action(error)),
        Ok(Err(V8RequestError::Transport(message))) => {
            V8_RESTARTS_TOTAL.fetch_add(1, Ordering::Relaxed);
            let mut guard = handle.lock().await;
            match std::mem::replace(&mut *guard, V8Slot::Empty) {
                V8Slot::Running(mut proc, _) => {
                    proc.force_terminate("transport-failure").await;
                }
                V8Slot::Retired => *guard = V8Slot::Retired,
                V8Slot::Empty => {}
            }
            Err(V8RequestError::Transport(message))
        }
        Err(_elapsed) => {
            V8_RESTARTS_TOTAL.fetch_add(1, Ordering::Relaxed);
            let mut guard = handle.lock().await;
            match std::mem::replace(&mut *guard, V8Slot::Empty) {
                V8Slot::Running(mut proc, _) => {
                    proc.force_terminate("request-timeout").await;
                }
                V8Slot::Retired => *guard = V8Slot::Retired,
                V8Slot::Empty => {}
            }
            Err(V8RequestError::Transport(format!(
                "V8 request timed out after {timeout:?}"
            )))
        }
    }
}

async fn with_process(
    handle: &V8ProcessHandle,
    timeout: Duration,
    action: &str,
    name: &str,
    script: &str,
) -> Result<String, String> {
    with_process_detailed(handle, timeout, action, name, script)
        .await
        .map_err(V8RequestError::into_message)
}

pub async fn v8_context_exists(handle: &V8ProcessHandle, name: &str) -> bool {
    with_process(handle, V8_REQUEST_TIMEOUT, "exists", name, "")
        .await
        .map(|v| v == "true")
        .unwrap_or(false)
}

pub async fn v8_context_create(
    handle: &V8ProcessHandle,
    name: &str,
    init_script: &str,
) -> Result<(), String> {
    with_process(handle, V8_REQUEST_TIMEOUT, "create", name, init_script)
        .await
        .map(|_| ())
}

pub async fn v8_context_eval(
    handle: &V8ProcessHandle,
    name: &str,
    script: &str,
) -> Result<String, String> {
    with_process(handle, V8_REQUEST_TIMEOUT, "eval", name, script).await
}

pub async fn v8_context_drop(handle: &V8ProcessHandle, name: &str) {
    let _ = with_process(handle, V8_REQUEST_TIMEOUT, "drop", name, "").await;
}

/// Root directory under which per-source Chromium profiles are stored. Prefers
/// the explicit `KANI_BROWSER_PROFILES_DIR`, then `$KANI_DATA_DIR/.browser-profiles`,
/// falling back to a temp-dir subfolder.
fn profiles_root() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("KANI_BROWSER_PROFILES_DIR")
        && !p.is_empty()
    {
        return std::path::PathBuf::from(p);
    }
    if let Ok(d) = std::env::var("KANI_DATA_DIR")
        && !d.is_empty()
    {
        return std::path::PathBuf::from(d).join(".browser-profiles");
    }
    std::env::temp_dir().join("kani-browser-profiles")
}

/// Maps a source key to a dedicated Chromium `userDataDir`, sanitizing the key
/// to a single safe path component so login/cookie state never leaks between
/// sources and no key can escape the profiles root.
pub fn profile_dir_for(source_key: &str) -> std::path::PathBuf {
    let mut sanitized: String = source_key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() || sanitized.chars().all(|c| c == '.') {
        sanitized = "__default__".to_string();
    }
    let dir = profiles_root().join(sanitized);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[derive(Debug, Clone)]
pub enum CapturePagePayloadError {
    Action { code: String, message: String },
    Transport(String),
}

impl std::fmt::Display for CapturePagePayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Action { code, message } => write!(f, "{message} [{code}]"),
            Self::Transport(message) => f.write_str(message),
        }
    }
}

pub async fn capture_page_payload_resilient(
    _handle: &V8ProcessHandle,
    http: &crate::http::SmartClient,
    page_url: &str,
    init_script: &str,
    timeout_ms: u32,
    source_key: Option<&str>,
    auto_scroll: bool,
) -> Result<String, CapturePagePayloadError> {
    if !http.solver_configured() {
        return Err(CapturePagePayloadError::Action {
            code: "solver_not_configured".to_string(),
            message: "Browser capture needs a solver. Set a solver URL in Settings > Advanced."
                .to_string(),
        });
    }

    match http
        .solver_capture(page_url, init_script, timeout_ms, source_key, auto_scroll)
        .await
    {
        Ok(payload) => {
            record_browser_solver_result(true);
            Ok(payload)
        }
        Err(error @ crate::http::SolverCaptureError::Unsupported) => {
            record_browser_solver_result(false);
            Err(CapturePagePayloadError::Action {
                code: "solver_incompatible".to_string(),
                message: error.to_string(),
            })
        }
        Err(error @ crate::http::SolverCaptureError::Unauthorized) => {
            record_browser_solver_result(false);
            Err(CapturePagePayloadError::Action {
                code: "solver_unauthorized".to_string(),
                message: error.to_string(),
            })
        }
        Err(error @ crate::http::SolverCaptureError::ScriptProducedNothing(_)) => {
            record_browser_solver_result(false);
            Err(CapturePagePayloadError::Action {
                code: "solver_capture_empty".to_string(),
                message: error.to_string(),
            })
        }
        Err(error @ crate::http::SolverCaptureError::MissingEgressGuard) => {
            record_browser_solver_result(false);
            Err(CapturePagePayloadError::Action {
                code: "solver_egress_guard_missing".to_string(),
                message: error.to_string(),
            })
        }
        Err(error @ crate::http::SolverCaptureError::Unreachable) => {
            record_browser_solver_result(false);
            Err(CapturePagePayloadError::Action {
                code: "solver_unreachable".to_string(),
                message: error.to_string(),
            })
        }
        Err(error) => {
            record_browser_solver_result(false);
            Err(CapturePagePayloadError::Action {
                code: "solver_error".to_string(),
                message: error.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn null_handle() -> V8ProcessHandle {
        new_handle()
    }

    async fn worker_pid(handle: &V8ProcessHandle) -> Option<u32> {
        match &*handle.lock().await {
            V8Slot::Running(process, _) => process.child.id(),
            V8Slot::Empty | V8Slot::Retired => None,
        }
    }

    /// Both "false" and "0" values are tested in a single serialised block to
    /// avoid parallel tests racing on the process-wide env var.
    async fn solver_stub(capabilities: Option<serde_json::Value>) -> wiremock::MockServer {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let mut index = serde_json::json!({ "msg": "ready" });
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
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok",
                "sessions": [],
                "solution": { "payload": "{\"ok\":true}" }
            })))
            .mount(&server)
            .await;
        server
    }

    fn action_code(error: &CapturePagePayloadError) -> String {
        match error {
            CapturePagePayloadError::Action { code, .. } => code.clone(),
            CapturePagePayloadError::Transport(message) => panic!("expected an action: {message}"),
        }
    }

    #[tokio::test]
    async fn capture_without_a_solver_says_so() {
        let handle = null_handle();
        let http = crate::http::SmartClient::new(None).unwrap();

        let error = capture_page_payload_resilient(
            &handle,
            &http,
            "https://site.test/browse",
            "passPayload('x')",
            1000,
            Some("src"),
            false,
        )
        .await
        .expect_err("no solver means no capture");

        assert_eq!(action_code(&error), "solver_not_configured");
    }

    #[tokio::test]
    async fn capture_through_a_capable_solver_returns_the_payload() {
        let server = solver_stub(Some(serde_json::json!([
            "kani.capture/2",
            "kani.egress-guard/1"
        ])))
        .await;
        let handle = null_handle();
        let http = crate::http::SmartClient::new(Some(server.uri() + "/v1")).unwrap();

        let payload = capture_page_payload_resilient(
            &handle,
            &http,
            "https://site.test/browse",
            "passPayload('x')",
            1000,
            Some("src"),
            false,
        )
        .await
        .expect("a capable solver captures");

        assert_eq!(payload, "{\"ok\":true}");
    }

    #[tokio::test]
    async fn capture_through_a_stock_solver_reports_incompatibility() {
        let server = solver_stub(None).await;
        let handle = null_handle();
        let http = crate::http::SmartClient::new(Some(server.uri() + "/v1")).unwrap();

        let error = capture_page_payload_resilient(
            &handle,
            &http,
            "https://site.test/browse",
            "passPayload('x')",
            1000,
            Some("src"),
            false,
        )
        .await
        .expect_err("a stock solver cannot capture");

        assert_eq!(action_code(&error), "solver_incompatible");
    }

    #[tokio::test]
    async fn capture_against_an_absent_solver_reports_unreachable() {
        let handle = null_handle();
        let http = crate::http::SmartClient::new(Some("http://127.0.0.1:1/v1".into())).unwrap();

        let error = capture_page_payload_resilient(
            &handle,
            &http,
            "https://site.test/browse",
            "passPayload('x')",
            1000,
            Some("src"),
            false,
        )
        .await
        .expect_err("an absent solver cannot capture");

        assert_eq!(action_code(&error), "solver_unreachable");
    }

    #[test]
    fn set_v8_debug_logging_toggles_flag() {
        set_v8_debug_logging(true);
        assert!(V8_DEBUG_LOGGING.load(Ordering::Relaxed));
        assert!(v8_debug_logging_enabled());
        set_v8_debug_logging(false);
        assert!(!V8_DEBUG_LOGGING.load(Ordering::Relaxed));
        assert!(!v8_debug_logging_enabled());
    }

    #[test]
    fn profile_dir_for_sanitizes_path_traversal() {
        let root = profiles_root();
        for key in ["../evil", "..", ".", "a/b/c", "..\\..\\x"] {
            let dir = profile_dir_for(key);
            assert_eq!(
                dir.parent(),
                Some(root.as_path()),
                "key {key:?} escaped the profiles root: {dir:?}"
            );
            let last = dir.file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                !last.contains('/') && !last.contains('\\') && last != ".." && last != ".",
                "key {key:?} produced unsafe component {last:?}"
            );
        }
    }

    #[tokio::test]
    async fn reap_if_idle_returns_false_on_empty_handle() {
        let handle = null_handle();
        assert!(!reap_if_idle(&handle, Duration::from_secs(0)).await);
    }

    #[tokio::test]
    async fn action_error_keeps_worker_reusable() {
        let handle = null_handle();
        assert!(!v8_context_exists(&handle, "missing-context").await);
        let before = worker_pid(&handle).await.expect("worker should be running");

        let error = v8_context_eval(&handle, "missing-context", "1 + 1")
            .await
            .expect_err("missing context should be an action error");
        assert!(error.contains("not found"));
        assert_eq!(worker_pid(&handle).await, Some(before));

        v8_context_create(&handle, "reusable-context", "globalThis.answer = 42")
            .await
            .expect("worker should accept a later request");
        assert_eq!(
            v8_context_eval(&handle, "reusable-context", "answer")
                .await
                .expect("eval after action error"),
            "42"
        );
        assert!(shutdown(&handle, "test-complete").await);
    }

    #[tokio::test]
    async fn reap_live_worker_allows_clean_restart() {
        let handle = null_handle();
        assert!(!v8_context_exists(&handle, "first-worker").await);
        assert!(reap_if_idle(&handle, Duration::ZERO).await);
        assert!(worker_pid(&handle).await.is_none());
        assert!(!v8_context_exists(&handle, "second-worker").await);
        assert!(worker_pid(&handle).await.is_some());
        assert!(shutdown(&handle, "test-complete").await);
    }

    #[tokio::test]
    async fn retired_worker_cannot_respawn() {
        let handle = null_handle();
        assert!(!v8_context_exists(&handle, "retired-worker").await);
        assert!(retire(&handle, "test-retire").await);
        let error = v8_context_create(&handle, "must-not-start", "globalThis.value = 1")
            .await
            .expect_err("retired owner must reject new workers");
        assert!(error.contains("retired"));
        assert!(worker_pid(&handle).await.is_none());
    }

    #[tokio::test]
    async fn broken_ipc_forces_cleanup_and_resets_handle() {
        let handle = null_handle();
        assert!(!v8_context_exists(&handle, "broken-worker").await);
        {
            let mut guard = handle.lock().await;
            let V8Slot::Running(process, _) = &mut *guard else {
                panic!("worker should be running");
            };
            process.child.kill().await.expect("kill mock worker");
        }

        let error = with_process(&handle, V8_REQUEST_TIMEOUT, "exists", "after-kill", "")
            .await
            .expect_err("closed IPC must be a transport failure");
        assert!(error.contains("write") || error.contains("read") || error.contains("closed"));
        assert!(matches!(*handle.lock().await, V8Slot::Empty));
    }

    #[tokio::test]
    async fn shutdown_timeout_forces_cleanup() {
        let handle = null_handle();
        assert!(!v8_context_exists(&handle, "shutdown-timeout").await);
        let process = {
            let mut guard = handle.lock().await;
            match std::mem::replace(&mut *guard, V8Slot::Empty) {
                V8Slot::Running(process, _) => process,
                V8Slot::Empty | V8Slot::Retired => panic!("worker should be running"),
            }
        };

        assert!(
            !process
                .shutdown_with_timeout("test-timeout", Duration::ZERO)
                .await
        );
        assert!(worker_pid(&handle).await.is_none());
    }

    #[test]
    fn profile_dir_for_is_stable_and_distinct() {
        assert_eq!(profile_dir_for("source-a"), profile_dir_for("source-a"));
        assert_ne!(profile_dir_for("source-a"), profile_dir_for("source-b"));
    }

    #[test]
    fn v8_config_default_values() {
        let cfg = V8Config::default();
        assert_eq!(cfg.max_memory_mb, 512);
        assert_eq!(cfg.idle_timeout_s, 300);
    }

    #[test]
    fn set_v8_config_roundtrips() {
        let restore = v8_config();
        set_v8_config(V8Config {
            max_memory_mb: 1024,
            idle_timeout_s: 120,
        });
        let cfg = v8_config();
        assert_eq!(cfg.max_memory_mb, 1024);
        assert_eq!(cfg.idle_timeout_s, 120);
        let stats = browser_stats();
        assert_eq!(stats.max_memory_mb, 1024);
        assert_eq!(stats.idle_timeout_s, 120);
        set_v8_config(restore);
    }

    #[tokio::test]
    async fn v8_context_does_not_exist_for_unknown_name() {
        let handle = null_handle();
        let exists = v8_context_exists(&handle, "no-such-ctx-test-only").await;
        assert!(!exists);
    }
}
