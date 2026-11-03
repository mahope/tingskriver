// Sidecar process lifecycle management for llama-helper
// Handles spawning, health checking, keep-alive, and graceful shutdown

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::{Mutex, RwLock};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use super::models;
use tokio_util::sync::CancellationToken;

#[path = "sidecar_transport.rs"]
mod transport;
use transport::Transport;

struct MonitorTasks {
    token: CancellationToken,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Default for MonitorTasks {
    fn default() -> Self {
        Self {
            token: CancellationToken::new(),
            tasks: Vec::new(),
        }
    }
}

// ============================================================================
// Sidecar State Management
// ============================================================================

/// Sidecar process manager with keep-alive and health monitoring
pub struct SidecarManager {
    /// Child process handle
    child_process: Arc<Mutex<Option<Child>>>,

    transport: Arc<Mutex<Option<Transport>>>,
    monitors: Arc<Mutex<MonitorTasks>>,
    lifecycle: Arc<Mutex<()>>,
    health_live: Arc<AtomicUsize>,
    idle_live: Arc<AtomicUsize>,
    owns_tasks: bool,

    /// Last activity timestamp
    last_activity: Arc<RwLock<Instant>>,

    /// Health status
    is_healthy: Arc<AtomicBool>,

    /// Shutdown flag
    should_shutdown: Arc<AtomicBool>,

    /// Active request count (for graceful shutdown)
    active_request_count: Arc<AtomicUsize>,

    /// Path to llama-helper binary
    helper_binary_path: PathBuf,

    /// Current model path (if loaded)
    current_model_path: Arc<RwLock<Option<PathBuf>>>,

    /// Idle timeout in seconds (configurable via env var)
    idle_timeout_secs: u64,
}

/// RAII guard for tracking active requests
/// Decrements the active request count when dropped
struct RequestGuard {
    counter: Arc<AtomicUsize>,
}

impl RequestGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self { counter }
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

impl SidecarManager {
    /// Create a new sidecar manager
    pub fn new(_app_data_dir: PathBuf) -> Result<Self> {
        let helper_binary_path = Self::resolve_helper_binary()?;
        Ok(Self::with_binary(helper_binary_path))
    }

    fn with_binary(helper_binary_path: PathBuf) -> Self {
        // Get idle timeout from env var or use default
        let idle_timeout_secs = std::env::var("LLAMA_IDLE_TIMEOUT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(models::DEFAULT_IDLE_TIMEOUT_SECS);

        log::info!(
            "SidecarManager initialized with idle timeout: {}s",
            idle_timeout_secs
        );
        log::info!("Helper binary path: {}", helper_binary_path.display());

        Self {
            child_process: Arc::new(Mutex::new(None)),
            transport: Arc::new(Mutex::new(None)),
            monitors: Arc::new(Mutex::new(MonitorTasks::default())),
            lifecycle: Arc::new(Mutex::new(())),
            health_live: Arc::new(AtomicUsize::new(0)),
            idle_live: Arc::new(AtomicUsize::new(0)),
            owns_tasks: true,
            last_activity: Arc::new(RwLock::new(Instant::now())),
            is_healthy: Arc::new(AtomicBool::new(false)),
            should_shutdown: Arc::new(AtomicBool::new(false)),
            active_request_count: Arc::new(AtomicUsize::new(0)),
            helper_binary_path,
            current_model_path: Arc::new(RwLock::new(None)),
            idle_timeout_secs,
        }
    }

    /// Resolve the path to llama-helper binary
    #[allow(clippy::disallowed_methods)] // Synchronous disk worker.
    fn resolve_helper_binary() -> Result<PathBuf> {
        // 1. Check environment variable (dev mode or manual override)
        if let Ok(env_path) = std::env::var("TINGSKRIVER_LLAMA_HELPER") {
            if !env_path.is_empty() {
                let path = PathBuf::from(env_path);
                if path.exists() {
                    log::info!(
                        "Using llama-helper from TINGSKRIVER_LLAMA_HELPER: {}",
                        path.display()
                    );
                    return Ok(path);
                }
            }
        }

        // In production, Tauri bundles the binary with target triple suffix
        // 2. Check relative to current executable (most reliable for AppImage/bundled apps)
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                log::info!(
                    "Searching for llama-helper relative to executable: {}",
                    exe_dir.display()
                );

                // Get the target triple (same logic as before)
                let target_triple = std::env::var("TARGET").unwrap_or_else(|_| {
                    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                    {
                        "x86_64-unknown-linux-gnu".to_string()
                    }
                    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
                    {
                        "aarch64-unknown-linux-gnu".to_string()
                    }
                    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
                    {
                        "x86_64-apple-darwin".to_string()
                    }
                    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                    {
                        "aarch64-apple-darwin".to_string()
                    }
                    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
                    {
                        "x86_64-pc-windows-msvc".to_string()
                    }
                    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
                    {
                        "aarch64-pc-windows-msvc".to_string()
                    }
                    #[cfg(not(any(
                        all(
                            target_os = "linux",
                            any(target_arch = "x86_64", target_arch = "aarch64")
                        ),
                        all(
                            target_os = "macos",
                            any(target_arch = "x86_64", target_arch = "aarch64")
                        ),
                        all(
                            target_os = "windows",
                            any(target_arch = "x86_64", target_arch = "aarch64")
                        )
                    )))]
                    {
                        "unknown".to_string()
                    }
                });

                let binary_name = if cfg!(windows) {
                    format!("llama-helper-{}.exe", target_triple)
                } else {
                    format!("llama-helper-{}", target_triple)
                };

                // Try exact match in exe dir
                let bundled = exe_dir.join(&binary_name);
                if bundled.exists() {
                    log::info!(
                        "Found exact match next to executable: {}",
                        bundled.display()
                    );
                    return Ok(bundled);
                }

                // Fuzzy match in exe dir
                log::info!("Attempting fuzzy match in exe dir: {}", exe_dir.display());
                if let Ok(entries) = std::fs::read_dir(exe_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.starts_with("llama-helper") && !name.ends_with(".d") {
                                log::info!(
                                    "Found fuzzy match next to executable: {}",
                                    path.display()
                                );
                                return Ok(path);
                            }
                        }
                    }
                }
            }
        }

        // 3. Check bundled resources (RESOURCE_DIR) - Fallback
        if let Ok(resource_dir) = std::env::var("RESOURCE_DIR") {
            log::info!(
                "Searching for llama-helper in RESOURCE_DIR: {}",
                resource_dir
            );
            let resource_path = PathBuf::from(&resource_dir);
            // Get the target triple again (or we could have shared it, but code duplication is safer for this tool usage)
            let target_triple = std::env::var("TARGET").unwrap_or_else(|_| {
                #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                {
                    "x86_64-unknown-linux-gnu".to_string()
                }
                // ... (abbreviated for brevity in thought, but must be full in tool)
                #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
                {
                    "aarch64-unknown-linux-gnu".to_string()
                }
                #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
                {
                    "x86_64-apple-darwin".to_string()
                }
                #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                {
                    "aarch64-apple-darwin".to_string()
                }
                #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
                {
                    "x86_64-pc-windows-msvc".to_string()
                }
                #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
                {
                    "aarch64-pc-windows-msvc".to_string()
                }
                #[cfg(not(any(
                    all(
                        target_os = "linux",
                        any(target_arch = "x86_64", target_arch = "aarch64")
                    ),
                    all(
                        target_os = "macos",
                        any(target_arch = "x86_64", target_arch = "aarch64")
                    ),
                    all(
                        target_os = "windows",
                        any(target_arch = "x86_64", target_arch = "aarch64")
                    )
                )))]
                {
                    "unknown".to_string()
                }
            });

            let binary_name = if cfg!(windows) {
                format!("llama-helper-{}.exe", target_triple)
            } else {
                format!("llama-helper-{}", target_triple)
            };

            let bundled = resource_path.join(&binary_name);
            if bundled.exists() {
                log::info!("Found exact match in RESOURCE_DIR: {}", bundled.display());
                return Ok(bundled);
            }

            // Fuzzy match in RESOURCE_DIR
            if let Ok(entries) = std::fs::read_dir(&resource_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with("llama-helper") && !name.ends_with(".d") {
                            log::info!("Found fuzzy match in RESOURCE_DIR: {}", path.display());
                            return Ok(path);
                        }
                    }
                }
            }
        } else {
            log::warn!("RESOURCE_DIR environment variable not set");
        }

        // 3. Fallback for dev: try relative paths from workspace (no target triple in dev builds)
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let project_root = PathBuf::from(&manifest_dir)
                .parent()
                .and_then(|p| p.parent())
                .ok_or_else(|| anyhow!("Failed to determine project root"))?
                .to_path_buf();

            let candidates = vec![
                project_root.join("target/release/llama-helper"),
                project_root.join("target/debug/llama-helper"),
                project_root.join("target/release/llama-helper.exe"),
                project_root.join("target/debug/llama-helper.exe"),
            ];

            for candidate in candidates {
                if candidate.exists() {
                    log::info!("Using dev llama-helper: {}", candidate.display());
                    return Ok(candidate);
                }
            }
        }

        Err(anyhow!(
            "llama-helper binary not found. Build with 'cd llama-helper && cargo build --release' or set TINGSKRIVER_LLAMA_HELPER env var."
        ))
    }

    /// Ensure sidecar is running, spawn if needed
    pub async fn ensure_running(&self, model_path: PathBuf) -> Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        // Check if already running with correct model
        {
            let current_model = self.current_model_path.read().await;
            if current_model.as_ref() == Some(&model_path) && self.is_healthy() {
                log::debug!("Sidecar already running with correct model");
                self.update_activity().await;
                return Ok(());
            }
        }

        // Need to spawn or restart
        self.spawn(model_path).await
    }

    /// Spawn the sidecar process
    async fn spawn(&self, model_path: PathBuf) -> Result<()> {
        // Shutdown existing process if running
        self.shutdown_inner().await?;
        self.stop_monitors().await;

        log::info!("Spawning llama-helper sidecar");
        log::info!("Model path: {}", model_path.display());

        #[cfg(unix)]
        let mut command = tokio::process::Command::new("nice");

        #[cfg(not(unix))]
        let mut command = tokio::process::Command::new(&self.helper_binary_path);

        #[cfg(unix)]
        command.arg("-n").arg("10").arg(&self.helper_binary_path);

        command
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Release builds have no console, so stderr goes to the log.
            .stderr(Stdio::piped())
            .env("LLAMA_IDLE_TIMEOUT", self.idle_timeout_secs.to_string());

        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;

            command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
        }

        let mut child = command.spawn().with_context(|| {
            format!(
                "Failed to spawn llama-helper at {:?}",
                self.helper_binary_path
            )
        })?;

        #[cfg(windows)]
        if let Some(pid) = child.id() {
            if let Err(error) = job::assign(pid) {
                log::warn!("Could not attach llama-helper to Job Object: {}", error);
            }
        }

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdout"))?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(log_helper_stderr(stderr));
        }

        // Store handles
        {
            let mut child_lock = self.child_process.lock().await;
            *child_lock = Some(child);
        }

        *self.transport.lock().await = Some(Transport::new(stdout, stdin));

        // Update state
        {
            let mut current_model = self.current_model_path.write().await;
            *current_model = Some(model_path);
        }

        self.is_healthy.store(true, Ordering::SeqCst);
        self.should_shutdown.store(false, Ordering::SeqCst);
        self.update_activity().await;

        log::info!("Sidecar spawned successfully");

        // Start background tasks
        self.start_monitors().await;

        Ok(())
    }

    /// Send a request to the sidecar and wait for response
    pub async fn send_request(&self, request_json: String, timeout: Duration) -> Result<String> {
        // Track active request
        let _guard = RequestGuard::new(self.active_request_count.clone());

        let mut exchange_started = false;
        let exchange = async {
            let mut transport = self.transport.lock().await;
            let transport = transport
                .as_mut()
                .ok_or_else(|| anyhow!("Sidecar not running"))?;
            exchange_started = true;
            transport.exchange(&request_json).await
        };
        match tokio::time::timeout(timeout, exchange).await {
            Ok(result) => {
                if result.is_ok() {
                    self.update_activity().await;
                } else {
                    self.is_healthy.store(false, Ordering::SeqCst);
                }
                result
            }
            Err(_) => {
                if exchange_started {
                    self.shutdown().await?;
                }
                Err(anyhow!("Request timed out after {:?}", timeout))
            }
        }
    }

    async fn stop_monitors(&self) {
        let tasks = {
            let mut monitors = self.monitors.lock().await;
            monitors.token.cancel();
            std::mem::take(&mut monitors.tasks)
        };
        for task in tasks {
            task.abort();
            let _ = task.await;
        }
    }

    async fn start_monitors(&self) {
        self.stop_monitors().await;
        let mut monitors = self.monitors.lock().await;
        monitors.token = CancellationToken::new();
        let token = monitors.token.clone();
        monitors
            .tasks
            .push(self.start_health_check_loop(token.clone()));
        monitors.tasks.push(self.start_idle_check_loop(token));
    }

    async fn send_ping(&self) -> Result<()> {
        // A busy generator owns the complete transaction. Never queue a health ping.
        let Ok(mut transport) = self.transport.try_lock() else {
            return Ok(());
        };
        let transport = transport
            .as_mut()
            .ok_or_else(|| anyhow!("Sidecar not running"))?;
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            transport.exchange(r#"{"type":"ping"}"#),
        )
        .await??;
        let response: serde_json::Value = serde_json::from_str(&response)?;
        if response["type"] == "pong" {
            Ok(())
        } else {
            Err(anyhow!("Helper rejected health ping"))
        }
    }

    /// Gracefully shutdown the sidecar
    /// Waits for active requests to complete before killing the process
    pub async fn shutdown_gracefully(&self) -> Result<()> {
        log::info!("Initiating graceful shutdown of sidecar");

        // Set shutdown flag to prevent new internal tasks
        self.should_shutdown.store(true, Ordering::SeqCst);

        // Wait for active requests to complete
        // We poll every 500ms
        let start = Instant::now();
        let max_wait = Duration::from_secs(600); // Wait up to 10 minutes for long generations

        loop {
            let count = self.active_request_count.load(Ordering::SeqCst);
            if count == 0 {
                log::info!("No active requests, proceeding with shutdown");
                break;
            }

            if start.elapsed() > max_wait {
                log::warn!(
                    "Timed out waiting for active requests ({} active), forcing shutdown",
                    count
                );
                break;
            }

            log::debug!("Waiting for {} active requests to complete...", count);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        self.shutdown().await
    }

    /// Force shutdown the sidecar
    pub async fn shutdown(&self) -> Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        self.shutdown_inner().await
    }

    async fn shutdown_inner(&self) -> Result<()> {
        // Set shutdown flag
        self.should_shutdown.store(true, Ordering::SeqCst);

        self.monitors.lock().await.token.cancel();

        // Kill process if still running
        {
            let mut child_lock = self.child_process.lock().await;
            if let Some(mut child) = child_lock.take() {
                child
                    .kill()
                    .await
                    .context("Failed to terminate llama-helper")?;
            }
        }

        *self.transport.lock().await = None;

        {
            let mut current_model = self.current_model_path.write().await;
            *current_model = None;
        }

        self.is_healthy.store(false, Ordering::SeqCst);

        log::info!("Sidecar shutdown complete");
        Ok(())
    }

    /// Check if sidecar is healthy
    pub fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::SeqCst)
    }

    /// Update last activity timestamp
    async fn update_activity(&self) {
        let mut last_activity = self.last_activity.write().await;
        *last_activity = Instant::now();
    }

    /// Get seconds since last activity
    async fn seconds_since_activity(&self) -> u64 {
        let last_activity = self.last_activity.read().await;
        last_activity.elapsed().as_secs()
    }

    /// Start health check loop (runs in background)
    fn start_health_check_loop(&self, token: CancellationToken) -> tokio::task::JoinHandle<()> {
        let manager = Self {
            child_process: self.child_process.clone(),
            transport: self.transport.clone(),
            monitors: self.monitors.clone(),
            lifecycle: self.lifecycle.clone(),
            health_live: self.health_live.clone(),
            idle_live: self.idle_live.clone(),
            owns_tasks: false,
            last_activity: self.last_activity.clone(),
            is_healthy: self.is_healthy.clone(),
            should_shutdown: self.should_shutdown.clone(),
            active_request_count: self.active_request_count.clone(),
            helper_binary_path: self.helper_binary_path.clone(),
            current_model_path: self.current_model_path.clone(),
            idle_timeout_secs: self.idle_timeout_secs,
        };

        tokio::spawn(async move {
            let _guard = RequestGuard::new(manager.health_live.clone());
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => break,
                    _ = interval.tick() => {}
                }

                if manager.should_shutdown.load(Ordering::SeqCst) {
                    log::debug!("Health check loop: shutdown flag set, exiting");
                    break;
                }

                if !manager.is_healthy() {
                    log::debug!("Health check loop: sidecar unhealthy, skipping ping");
                    continue;
                }

                // Don't ping if we are busy with a request
                if manager.active_request_count.load(Ordering::SeqCst) > 0 {
                    continue;
                }

                log::debug!("Health check: sending ping");
                let ping = tokio::select! {
                    biased;
                    _ = token.cancelled() => break,
                    result = manager.send_ping() => result,
                };
                if let Err(e) = ping {
                    log::warn!("Health check failed: {}", e);
                    manager.is_healthy.store(false, Ordering::SeqCst);
                }
            }

            log::debug!("Health check loop exited");
        })
    }

    /// Start idle check loop (runs in background)
    fn start_idle_check_loop(&self, token: CancellationToken) -> tokio::task::JoinHandle<()> {
        let manager = Self {
            child_process: self.child_process.clone(),
            transport: self.transport.clone(),
            monitors: self.monitors.clone(),
            lifecycle: self.lifecycle.clone(),
            health_live: self.health_live.clone(),
            idle_live: self.idle_live.clone(),
            owns_tasks: false,
            last_activity: self.last_activity.clone(),
            is_healthy: self.is_healthy.clone(),
            should_shutdown: self.should_shutdown.clone(),
            active_request_count: self.active_request_count.clone(),
            helper_binary_path: self.helper_binary_path.clone(),
            current_model_path: self.current_model_path.clone(),
            idle_timeout_secs: self.idle_timeout_secs,
        };

        tokio::spawn(async move {
            let _guard = RequestGuard::new(manager.idle_live.clone());
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => break,
                    _ = interval.tick() => {}
                }

                if manager.should_shutdown.load(Ordering::SeqCst) {
                    log::debug!("Idle check loop: shutdown flag set, exiting");
                    break;
                }

                // Don't shutdown if we are busy
                if manager.active_request_count.load(Ordering::SeqCst) > 0 {
                    // Update activity to prevent timeout immediately after request finishes
                    manager.update_activity().await;
                    continue;
                }

                let idle_secs = manager.seconds_since_activity().await;
                log::debug!("Idle check: {}s since last activity", idle_secs);

                if idle_secs > manager.idle_timeout_secs {
                    log::info!(
                        "Sidecar idle for {}s (timeout: {}s), shutting down",
                        idle_secs,
                        manager.idle_timeout_secs
                    );

                    if let Err(e) = manager.shutdown().await {
                        log::error!("Failed to shutdown idle sidecar: {}", e);
                    }

                    break;
                }
            }

            log::debug!("Idle check loop exited");
        })
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        if !self.owns_tasks {
            return;
        }
        if let Ok(monitors) = self.monitors.try_lock() {
            monitors.token.cancel();
        }
        self.should_shutdown.store(true, Ordering::SeqCst);

        // Note: Actual cleanup happens in shutdown() method
        // We can't do async work in Drop, so this is best-effort
        log::debug!("SidecarManager dropped");
    }
}

/// Write each line the helper prints on stderr (model loading, VRAM,
/// generation statistics; never prompt or output text) to the log.
async fn log_helper_stderr(stderr: tokio::process::ChildStderr) {
    let mut lines = BufReader::new(stderr).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => log::info!("llama-helper: {}", line),
            Ok(None) => break,
            Err(e) => {
                log::warn!("llama-helper: could not read stderr: {}", e);
                break;
            }
        }
    }
}

#[cfg(windows)]
#[path = "sidecar_job.rs"]
mod job;

#[cfg(test)]
#[path = "sidecar_tests.rs"]
mod tests;
