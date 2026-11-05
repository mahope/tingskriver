use crate::parakeet_engine::pool::{self, BoxedModel, ModelPool};
use crate::model_download::{self, FileEntry, ProgressSink, Source, TransferError, TransferOptions};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::fs;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex, RwLock};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

/// Quantization type for Parakeet models
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum QuantizationType {
    FP32,   // Full precision
    Int8,   // 8-bit integer quantization (faster)
}

impl Default for QuantizationType {
    fn default() -> Self {
        QuantizationType::Int8 // Default to int8 for best performance
    }
}

/// Model status for Parakeet models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelStatus {
    Available,
    Missing,
    Downloading { progress: u8 },
    Error(String),
    Corrupted { file_size: u64, expected_min_size: u64 },
}

/// Detailed download progress info (MB-based with speed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    /// Bytes downloaded so far
    pub downloaded_bytes: u64,
    /// Total file size in bytes
    pub total_bytes: u64,
    /// Downloaded in MB (for display)
    pub downloaded_mb: f64,
    /// Total size in MB (for display)
    pub total_mb: f64,
    /// Download speed in MB/s
    pub speed_mbps: f64,
    /// Percentage complete (0-100)
    pub percent: u8,
}

impl DownloadProgress {
    pub fn new(downloaded: u64, total: u64, speed_mbps: f64) -> Self {
        let percent = if total > 0 {
            ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u8
        } else {
            0
        };
        Self {
            downloaded_bytes: downloaded,
            total_bytes: total,
            downloaded_mb: downloaded as f64 / (1024.0 * 1024.0),
            total_mb: total as f64 / (1024.0 * 1024.0),
            speed_mbps,
            percent,
        }
    }
}

/// Information about a Parakeet model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_mb: u32,
    pub quantization: QuantizationType,
    pub speed: String,     // Performance description
    pub status: ModelStatus,
    pub description: String,
}

struct ModelSpec {
    name: &'static str,
    size_mb: u32,
    quantization: QuantizationType,
    speed: &'static str,
    description: &'static str,
}

/// Files, sizes, hashes and sources come from the pinned manifest
/// (`model_manifest.json`); the rest of a model's description lives here.
fn manifest_files(model_name: &str) -> Vec<FileEntry> {
    model_download::entry("parakeet", model_name)
        .map(|entry| entry.files.clone())
        .unwrap_or_default()
}

const PARAKEET_MODEL_SPECS: &[ModelSpec] = &[
    ModelSpec {
        name: "parakeet-tdt-0.6b-v3-int8",
        size_mb: 670,
        quantization: QuantizationType::Int8,
        speed: "Ultra Fast (v3)",
        description: "Real time on M4 Max, latest version with int8 quantization",
    },
    ModelSpec {
        name: "parakeet-tdt-0.6b-v2-int8",
        size_mb: 661,
        quantization: QuantizationType::Int8,
        speed: "Fast (v2)",
        description: "Previous version with int8 quantization, good balance of speed and accuracy",
    },
];

fn find_model_spec(model_name: &str) -> Option<&'static ModelSpec> {
    PARAKEET_MODEL_SPECS
        .iter()
        .find(|spec| spec.name == model_name)
}

struct ActiveDownload {
    cancellation: CancellationToken,
    completion: watch::Sender<bool>,
    /// A complete model was already on disk when this download started; a failed
    /// or cancelled download leaves it available.
    had_complete_model: AtomicBool,
}

#[derive(Default)]
struct ActiveDownloadState {
    downloads: HashMap<String, Arc<ActiveDownload>>,
    revision: u64,
}

#[derive(Debug, thiserror::Error)]
#[error("Download cancelled by user")]
pub(crate) struct DownloadCancelled;

pub(crate) fn is_download_cancelled(error: &anyhow::Error) -> bool {
    error.downcast_ref::<DownloadCancelled>().is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CancelDownloadOutcome {
    Cancelled,
    Pending,
}

const CANCEL_DOWNLOAD_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
struct DownloadStateTestHook {
    finalization_ready: tokio::sync::Notify,
    continue_finalization: tokio::sync::Notify,
    discovery_scanned: tokio::sync::Notify,
    continue_discovery: tokio::sync::Notify,
}

#[cfg(test)]
struct ModelLifecycleTestHook {
    load_started: tokio::sync::Notify,
    continue_load: tokio::sync::Notify,
    unload_attempted: tokio::sync::Notify,
}

#[derive(Debug)]
pub enum ParakeetEngineError {
    ModelNotLoaded,
    ModelNotFound(String),
    TranscriptionFailed(String),
    DownloadFailed(String),
    IoError(std::io::Error),
    Other(String),
}

impl std::fmt::Display for ParakeetEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParakeetEngineError::ModelNotLoaded => write!(f, "No Parakeet model loaded"),
            ParakeetEngineError::ModelNotFound(name) => write!(f, "Model '{}' not found", name),
            ParakeetEngineError::TranscriptionFailed(err) => write!(f, "Transcription failed: {}", err),
            ParakeetEngineError::DownloadFailed(err) => write!(f, "Download failed: {}", err),
            ParakeetEngineError::IoError(err) => write!(f, "IO error: {}", err),
            ParakeetEngineError::Other(err) => write!(f, "Error: {}", err),
        }
    }
}

impl std::error::Error for ParakeetEngineError {}

impl From<std::io::Error> for ParakeetEngineError {
    fn from(err: std::io::Error) -> Self {
        ParakeetEngineError::IoError(err)
    }
}

/// Turns byte counts from the shared downloader into Parakeet's progress callbacks
/// and model status.
struct ParakeetProgress {
    models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    model_name: String,
    callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    base_bytes: u64,
    total_bytes: u64,
    last_file_bytes: u64,
    bytes_since_report: u64,
    last_percent: u8,
    last_report: Instant,
    first: bool,
}

#[async_trait]
impl ProgressSink for ParakeetProgress {
    async fn bytes(&mut self, file_bytes: u64) {
        self.bytes_since_report = self
            .bytes_since_report
            .saturating_add(file_bytes.saturating_sub(self.last_file_bytes));
        self.last_file_bytes = file_bytes;
        let confirmed = self.base_bytes.saturating_add(file_bytes).min(self.total_bytes);
        let progress = ParakeetEngine::in_flight_progress(confirmed, self.total_bytes, 0.0);
        let elapsed = self.last_report.elapsed();
        if !(self.first || progress.percent > self.last_percent || elapsed >= Duration::from_millis(500)) {
            return;
        }
        let speed_mbps = if !self.first && elapsed.as_secs_f64() > 0.0 {
            self.bytes_since_report as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64()
        } else {
            0.0
        };
        if let Some(callback) = &self.callback {
            callback(ParakeetEngine::in_flight_progress(confirmed, self.total_bytes, speed_mbps));
        }
        if let Some(model) = self.models.write().await.get_mut(&self.model_name) {
            model.status = ModelStatus::Downloading { progress: progress.percent };
        }
        self.first = false;
        self.last_percent = progress.percent;
        self.last_report = Instant::now();
        self.bytes_since_report = 0;
    }
}

pub struct ParakeetEngine {
    models_dir: PathBuf,
    /// The read lock is only held to clone the `Arc`, never during inference.
    current_model: Arc<RwLock<Option<Arc<ModelPool>>>>,
    current_model_name: Arc<RwLock<Option<String>>>,
    model_lifecycle_lock: Mutex<()>,
    pub(crate) available_models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    active_downloads: Arc<Mutex<ActiveDownloadState>>,
    #[cfg(test)]
    download_state_test_hook: Mutex<Option<Arc<DownloadStateTestHook>>>,
    #[cfg(test)]
    model_lifecycle_test_hook: Mutex<Option<Arc<ModelLifecycleTestHook>>>,
}

impl ParakeetEngine {
    /// Create a new Parakeet engine with optional custom models directory
    #[allow(clippy::disallowed_methods)]
    pub fn new_with_models_dir(models_dir: Option<PathBuf>) -> Result<Self> {
        let models_dir = if let Some(dir) = models_dir {
            dir.join("parakeet") // Parakeet models in subdirectory
        } else {
            // Fallback to default location
            let current_dir = std::env::current_dir()
                .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;

            if cfg!(debug_assertions) {
                // Development mode
                current_dir.join("models").join("parakeet")
            } else {
                // Production mode
                dirs::data_dir()
                    .or_else(|| dirs::home_dir())
                    .ok_or_else(|| anyhow!("Could not find system data directory"))?
                    .join("Meetily")
                    .join("models")
                    .join("parakeet")
            }
        };

        log::info!("ParakeetEngine using models directory: {}", models_dir.display());

        // Create directory if it doesn't exist
        if !models_dir.exists() {
            std::fs::create_dir_all(&models_dir)?;
        }

        Ok(Self {
            models_dir,
            current_model: Arc::new(RwLock::new(None)),
            current_model_name: Arc::new(RwLock::new(None)),
            model_lifecycle_lock: Mutex::new(()),
            available_models: Arc::new(RwLock::new(HashMap::new())),
            active_downloads: Arc::new(Mutex::new(ActiveDownloadState::default())),
            #[cfg(test)]
            download_state_test_hook: Mutex::new(None),
            #[cfg(test)]
            model_lifecycle_test_hook: Mutex::new(None),
        })
    }

    #[cfg(test)]
    async fn test_hook(&self) -> Option<Arc<DownloadStateTestHook>> {
        self.download_state_test_hook.lock().await.clone()
    }

    #[cfg(test)]
    async fn load_test_hook(&self) -> Option<Arc<ModelLifecycleTestHook>> {
        self.model_lifecycle_test_hook.lock().await.clone()
    }

    /// Discover available Parakeet models.
    pub async fn discover_models(&self) -> Result<Vec<ModelInfo>> {
        self.discover_models_from_specs(PARAKEET_MODEL_SPECS, &manifest_files).await
    }

    async fn discover_models_from_specs(
        &self,
        specs: &[ModelSpec],
        files_of: &(dyn Fn(&str) -> Vec<FileEntry> + Sync),
    ) -> Result<Vec<ModelInfo>> {
        loop {
            let revision = self.active_downloads.lock().await.revision;
            let mut models = Vec::with_capacity(specs.len());
            let mut validation_errors = Vec::new();

            for spec in specs {
                let model_path = self.models_dir.join(spec.name);
                let status = if model_path.exists() {
                    let validation_path = model_path.clone();
                    let artifacts = files_of(spec.name);
                    match tokio::task::spawn_blocking(move || {
                        Self::validate_model_directory(&validation_path, &artifacts)
                    })
                    .await
                    .map_err(|e| anyhow!("Model validation task failed: {e}"))?
                    {
                        Ok(()) => ModelStatus::Available,
                        Err(error) => {
                            let mut file_size = 0;
                            let expected_bytes: u64 = files_of(spec.name).iter().map(|a| a.size).sum();
                            for artifact in &files_of(spec.name) {
                                if let Ok(metadata) =
                                    tokio::fs::metadata(model_path.join(&artifact.name)).await
                                {
                                    file_size += metadata.len();
                                }
                            }
                            validation_errors.push((spec.name, error));
                            ModelStatus::Corrupted {
                                file_size,
                                expected_min_size: expected_bytes,
                            }
                        }
                    }
                } else {
                    ModelStatus::Missing
                };

                models.push(ModelInfo {
                    name: spec.name.to_string(),
                    path: model_path,
                    size_mb: spec.size_mb,
                    quantization: spec.quantization,
                    speed: spec.speed.to_string(),
                    status,
                    description: spec.description.to_string(),
                });
            }

            #[cfg(test)]
            if let Some(hook) = self.test_hook().await {
                hook.discovery_scanned.notify_one();
                hook.continue_discovery.notified().await;
            }

            let active_downloads = self.active_downloads.lock().await;
            if active_downloads.revision != revision {
                continue;
            }

            validation_errors.retain(|(model_name, _)| {
                !active_downloads.downloads.contains_key(*model_name)
            });
            for model in &mut models {
                if active_downloads.downloads.contains_key(&model.name) {
                    model.status = ModelStatus::Downloading { progress: 0 };
                }
            }

            let mut available_models = self.available_models.write().await;
            available_models.clear();
            for model in &models {
                available_models.insert(model.name.clone(), model.clone());
            }
            drop(available_models);
            drop(active_downloads);

            for (model_name, error) in validation_errors {
                log::warn!("Model directory {} appears corrupted: {}", model_name, error);
            }
            return Ok(models);
        }
    }

    #[allow(clippy::disallowed_methods)]
    fn validate_model_directory(model_dir: &Path, artifacts: &[FileEntry]) -> Result<()> {
        for artifact in artifacts {
            let path = model_dir.join(&artifact.name);
            let metadata = std::fs::metadata(&path)
                .map_err(|error| anyhow!("Failed to read {} metadata: {}", artifact.name, error))?;
            if metadata.len() != artifact.size {
                return Err(anyhow!(
                    "{} has {} bytes, expected exactly {} bytes",
                    artifact.name,
                    metadata.len(),
                    artifact.size
                ));
            }
        }

        Ok(())
    }

    /// Load a Parakeet model
    pub async fn load_model(&self, model_name: &str) -> Result<()> {
        let model_info = {
            let models = self.available_models.read().await;
            models
                .get(model_name)
                .cloned()
                .ok_or_else(|| anyhow!("Model {} not found", model_name))?
        };

        match &model_info.status {
            ModelStatus::Available => {
                self.verify_installed_model(model_name, &model_info.path).await?;
                let _lifecycle_guard = self.model_lifecycle_lock.lock().await;
                let current_model = self.current_model_name.read().await.clone();
                if current_model.as_deref() == Some(model_name) {
                    log::info!("Parakeet model {} is already loaded, skipping reload", model_name);
                    return Ok(());
                }

                if let Some(current_model) = current_model {
                    log::info!(
                        "Unloading current Parakeet model '{}' before loading '{}'",
                        current_model,
                        model_name
                    );
                }
                self.unload_model_locked().await;

                log::info!("Loading Parakeet model: {}", model_name);

                let quantized = model_info.quantization == QuantizationType::Int8;
                let model_path = model_info.path.clone();
                #[cfg(test)]
                let model_lifecycle_test_hook = self.load_test_hook().await;
                #[cfg(test)]
                let runtime_handle = tokio::runtime::Handle::current();
                let model = tokio::task::spawn_blocking(move || {
                    #[cfg(test)]
                    if let Some(hook) = model_lifecycle_test_hook {
                        hook.load_started.notify_one();
                        runtime_handle.block_on(hook.continue_load.notified());
                    }
                    crate::stt::RemoteModel::load(&model_path, quantized)
                        .map(|model| Box::new(model) as BoxedModel)
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| {
                    anyhow!(
                        "Parakeet model load task failed for {}: {}",
                        model_name,
                        error
                    )
                })?
                .map_err(|error| {
                    anyhow!("Failed to load Parakeet model {}: {}", model_name, error)
                })?;

                let loader_path = model_info.path.clone();
                let loader: pool::Loader = Box::new(move || {
                    crate::stt::RemoteModel::load(&loader_path, quantized)
                        .map(|model| Box::new(model) as BoxedModel)
                        .map_err(|error| error.to_string())
                });
                *self.current_model.write().await =
                    Some(Arc::new(ModelPool::new(model, loader, pool::MAX_INSTANCES)));
                *self.current_model_name.write().await = Some(model_name.to_string());

                log::info!(
                    "Successfully loaded Parakeet model: {} ({})",
                    model_name,
                    if quantized { "Int8 quantized" } else { "FP32" }
                );
                Ok(())
            }
            ModelStatus::Missing => {
                Err(anyhow!("Parakeet model {} is not downloaded", model_name))
            }
            ModelStatus::Downloading { .. } => {
                Err(anyhow!("Parakeet model {} is currently downloading", model_name))
            }
            ModelStatus::Error(err) => {
                Err(anyhow!("Parakeet model {} has error: {}", model_name, err))
            }
            ModelStatus::Corrupted { .. } => {
                Err(anyhow!("Parakeet model {} is corrupted and cannot be loaded", model_name))
            }
        }
    }

    /// Check the SHA-256 of an installed model against the manifest before it is loaded.
    /// A file that no longer matches is never loaded; the model is marked corrupted.
    async fn verify_installed_model(&self, model_name: &str, model_dir: &Path) -> Result<()> {
        let Some(entry) = model_download::entry("parakeet", model_name) else {
            return Ok(());
        };
        let cancel = CancellationToken::new();
        for file in &entry.files {
            if let Err(error) = model_download::verify_installed(model_dir, file, &cancel).await {
                let file_size = fs::metadata(model_dir.join(&file.name))
                    .await
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                if let Some(model) = self.available_models.write().await.get_mut(model_name) {
                    model.status = ModelStatus::Corrupted {
                        file_size,
                        expected_min_size: file.size,
                    };
                }
                return Err(anyhow::Error::from(error));
            }
        }
        Ok(())
    }

    /// Unload the current model
    pub async fn unload_model(&self) -> bool {
        #[cfg(test)]
        if let Some(hook) = self.load_test_hook().await {
            hook.unload_attempted.notify_one();
        }
        let _lifecycle_guard = self.model_lifecycle_lock.lock().await;
        self.unload_model_locked().await
    }

    async fn unload_model_locked(&self) -> bool {
        let unloaded = self.current_model.write().await.take().is_some();
        if unloaded {
            log::info!("Parakeet model unloaded");
        }
        self.current_model_name.write().await.take();
        unloaded
    }

    /// Get the currently loaded model name
    pub async fn get_current_model(&self) -> Option<String> {
        self.current_model_name.read().await.clone()
    }

    /// Check if a model is loaded
    pub async fn is_model_loaded(&self) -> bool {
        self.current_model.read().await.is_some()
    }

    /// Transcribe audio samples using the loaded Parakeet model.
    ///
    /// Runs on a blocking thread with its own model instance, so a short
    /// dictation does not wait behind a long meeting segment, and loading,
    /// unloading and status checks are never blocked by inference.
    pub async fn transcribe_audio(&self, audio_data: Vec<f32>) -> Result<String> {
        let pool = self
            .current_model
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("No Parakeet model loaded. Please load a model first."))?;

        let duration_seconds = audio_data.len() as f64 / 16000.0; // Assuming 16kHz
        log::debug!(
            "Parakeet transcribing {} samples ({:.1}s duration)",
            audio_data.len(),
            duration_seconds
        );

        let text = tokio::task::spawn_blocking(move || pool.transcribe(audio_data))
            .await
            .map_err(|e| anyhow!("Parakeet transcription task failed: {}", e))?
            .map_err(|e| anyhow!("Parakeet transcription failed: {}", e))?;

        log::debug!("Parakeet transcription result: {} bytes", text.len());

        Ok(text)
    }

    pub async fn transcribe_audio_timed(
        &self,
        audio_data: Vec<f32>,
    ) -> Result<Vec<crate::stt::TimedWord>> {
        let pool = self
            .current_model
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("No Parakeet model loaded"))?;
        tokio::task::spawn_blocking(move || pool.transcribe_timed(audio_data))
            .await?
            .map_err(anyhow::Error::msg)
    }

    /// When the loaded model last finished a call, and how many calls run now.
    /// `None` when no model is loaded.
    pub async fn usage(&self) -> Option<(Instant, usize)> {
        let pool = self.current_model.read().await.clone()?;
        Some((pool.last_used(), pool.busy()))
    }

    /// Count the loaded model as used now (a dictation is about to need it).
    pub async fn mark_used(&self) {
        if let Some(pool) = self.current_model.read().await.as_ref() {
            pool.mark_used();
        }
    }

    #[cfg(test)]
    pub(crate) async fn set_test_pool(&self, name: &str, pool: ModelPool) {
        *self.current_model.write().await = Some(Arc::new(pool));
        *self.current_model_name.write().await = Some(name.to_string());
    }

    /// Get the models directory path
    pub async fn get_models_directory(&self) -> PathBuf {
        self.models_dir.clone()
    }

    /// Delete a corrupted model
    pub async fn delete_model(&self, model_name: &str) -> Result<String> {
        log::info!("Attempting to delete Parakeet model: {}", model_name);

        // Get model info to find the directory path
        let model_info = {
            let models = self.available_models.read().await;
            models.get(model_name).cloned()
        };

        let model_info = model_info.ok_or_else(|| anyhow!("Parakeet model '{}' not found", model_name))?;

        log::info!("Parakeet model '{}' has status: {:?}", model_name, model_info.status);

        // Allow deletion of corrupted or available models
        match &model_info.status {
            ModelStatus::Corrupted { .. } | ModelStatus::Available => {
                // Delete the entire model directory
                if model_info.path.exists() {
                    fs::remove_dir_all(&model_info.path).await
                        .map_err(|e| anyhow!("Failed to delete directory '{}': {}", model_info.path.display(), e))?;
                    log::info!("Successfully deleted Parakeet model directory: {}", model_info.path.display());
                } else {
                    log::warn!("Directory '{}' does not exist, nothing to delete", model_info.path.display());
                }

                // Update model status to Missing
                {
                    let mut models = self.available_models.write().await;
                    if let Some(model) = models.get_mut(model_name) {
                        model.status = ModelStatus::Missing;
                    }
                }

                Ok(format!("Successfully deleted Parakeet model '{}'", model_name))
            }
            _ => {
                Err(anyhow!(
                    "Can only delete corrupted or available Parakeet models. Model '{}' has status: {:?}",
                    model_name,
                    model_info.status
                ))
            }
        }
    }

    /// Download a Parakeet model from HuggingFace (backward-compatible wrapper).
    pub async fn download_model(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
    ) -> Result<()> {
        let detailed_callback = progress_callback.map(|callback| {
            Box::new(move |progress: DownloadProgress| callback(progress.percent))
                as Box<dyn Fn(DownloadProgress) + Send>
        });
        self.download_model_detailed(model_name, detailed_callback).await
    }

    /// Download a catalogued Parakeet model with detailed progress.
    pub async fn download_model_detailed(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        let model_info = self
            .available_models
            .read()
            .await
            .get(model_name)
            .cloned()
            .ok_or_else(|| anyhow!("Model {} not found", model_name))?;
        find_model_spec(model_name)
            .ok_or_else(|| anyhow!("Unsupported Parakeet model: {}", model_name))?;
        let entry = model_download::entry("parakeet", model_name)
            .ok_or_else(|| anyhow!("Parakeet model {} is missing from the model manifest", model_name))?;

        self.download_model_detailed_from_source(
            model_name,
            &model_info.path,
            &entry.sources,
            &entry.files,
            progress_callback,
        )
        .await
    }

    async fn reserve_active_download(&self, model_name: &str) -> Result<Arc<ActiveDownload>> {
        let mut active_downloads = self.active_downloads.lock().await;
        if active_downloads.downloads.contains_key(model_name) {
            return Err(anyhow!("Download already in progress for model: {}", model_name));
        }

        let (completion, _) = watch::channel(false);
        let active_download = Arc::new(ActiveDownload {
            cancellation: CancellationToken::new(),
            completion,
            had_complete_model: AtomicBool::new(false),
        });
        active_downloads
            .downloads
            .insert(model_name.to_string(), Arc::clone(&active_download));
        active_downloads.revision = active_downloads.revision.wrapping_add(1);
        Ok(active_download)
    }

    async fn set_downloading_status(&self, model_name: &str, progress: u8) {
        let mut models = self.available_models.write().await;
        if let Some(model) = models.get_mut(model_name) {
            model.status = ModelStatus::Downloading { progress };
        }
    }

    fn in_flight_progress(
        downloaded_bytes: u64,
        total_bytes: u64,
        speed_mbps: f64,
    ) -> DownloadProgress {
        let mut progress = DownloadProgress::new(downloaded_bytes, total_bytes, speed_mbps);
        progress.percent = progress.percent.min(99);
        progress
    }


    async fn download_model_detailed_from_source(
        &self,
        model_name: &str,
        model_dir: &Path,
        sources: &[Source],
        artifacts: &[FileEntry],
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        let active_download = self.reserve_active_download(model_name).await?;
        active_download.had_complete_model.store(
            Self::validate_model_directory(model_dir, artifacts).is_ok(),
            Ordering::SeqCst,
        );
        self.set_downloading_status(model_name, 0).await;

        let result = self
            .download_artifacts(
                model_name,
                model_dir,
                sources,
                artifacts,
                &active_download,
                progress_callback,
            )
            .await;
        let (result, progress_callback) = match result {
            Ok((progress, progress_callback)) => (Ok(progress), progress_callback),
            Err(error) => (Err(error), None),
        };

        self.finish_download(
            model_name,
            model_dir,
            artifacts,
            &active_download,
            result,
            progress_callback,
        )
        .await
    }

    /// Fetch every file of a model into `model_dir` through the verified downloader:
    /// `.part` file, Range resume, SHA-256 check, rename. Sources are tried in order.
    async fn download_artifacts(
        &self,
        model_name: &str,
        model_dir: &Path,
        sources: &[Source],
        artifacts: &[FileEntry],
        active_download: &ActiveDownload,
        mut progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<(DownloadProgress, Option<Box<dyn Fn(DownloadProgress) + Send>>)> {
        if active_download.cancellation.is_cancelled() {
            return Err(DownloadCancelled.into());
        }
        fs::create_dir_all(model_dir)
            .await
            .map_err(|error| anyhow!("Failed to create model directory: {}", error))?;

        let client = model_download::client()?;
        let options = TransferOptions::default();
        let total_bytes: u64 = artifacts.iter().map(|artifact| artifact.size).sum();
        let download_started = Instant::now();
        let mut confirmed_bytes = 0u64;
        let mut streamed_bytes = 0u64;

        for artifact in artifacts {
            if active_download.cancellation.is_cancelled() {
                return Err(DownloadCancelled.into());
            }

            let mut sink = ParakeetProgress {
                models: Arc::clone(&self.available_models),
                model_name: model_name.to_string(),
                callback: progress_callback.take(),
                base_bytes: confirmed_bytes,
                total_bytes,
                last_file_bytes: 0,
                bytes_since_report: 0,
                last_percent: Self::in_flight_progress(confirmed_bytes, total_bytes, 0.0).percent,
                last_report: Instant::now(),
                first: true,
            };
            let fetched = model_download::fetch_file(
                &client,
                model_dir,
                artifact,
                sources,
                &active_download.cancellation,
                &options,
                &mut sink,
            )
            .await;
            progress_callback = sink.callback.take();
            let fetched = fetched.map_err(|error| match error {
                TransferError::Cancelled => anyhow::Error::from(DownloadCancelled),
                other => anyhow::Error::from(other),
            })?;
            if !fetched.already_installed {
                log::info!(
                    "Parakeet model {}: {} came from source '{}'",
                    model_name,
                    artifact.name,
                    fetched.source
                );
                streamed_bytes = streamed_bytes.saturating_add(artifact.size);
            }
            confirmed_bytes = confirmed_bytes
                .checked_add(artifact.size)
                .ok_or_else(|| anyhow!("Progress overflow while downloading {}", artifact.name))?;
        }

        if confirmed_bytes != total_bytes {
            return Err(anyhow!(
                "Download confirmed {} bytes, expected {} bytes",
                confirmed_bytes,
                total_bytes
            ));
        }
        let elapsed = download_started.elapsed().as_secs_f64();
        let speed_mbps = if elapsed > 0.0 {
            streamed_bytes as f64 / (1024.0 * 1024.0) / elapsed
        } else {
            0.0
        };
        Ok((
            DownloadProgress::new(total_bytes, total_bytes, speed_mbps),
            progress_callback,
        ))
    }

    async fn finish_download(
        &self,
        model_name: &str,
        model_dir: &Path,
        artifacts: &[FileEntry],
        active_download: &Arc<ActiveDownload>,
        mut result: Result<DownloadProgress>,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        if result.is_ok() && !active_download.cancellation.is_cancelled() {
            let final_progress = result.expect("successful transfer must carry final progress");
            result = Self::validate_model_directory(model_dir, artifacts).map(|()| final_progress);
        }

        #[cfg(test)]
        if let Some(hook) = self.test_hook().await {
            hook.finalization_ready.notify_one();
            hook.continue_finalization.notified().await;
        }

        let mut active_downloads = self.active_downloads.lock().await;
        let owner_matches = matches!(
            active_downloads.downloads.get(model_name),
            Some(owner) if Arc::ptr_eq(owner, active_download)
        );
        if !owner_matches {
            drop(active_downloads);
            active_download.completion.send_replace(true);
            return result.map(|_| ());
        }

        let cancellation_won = active_download.cancellation.is_cancelled()
            || result
                .as_ref()
                .err()
                .is_some_and(is_download_cancelled);
        let existing_model_is_complete = active_download.had_complete_model.load(Ordering::SeqCst);
        let mut models = self.available_models.write().await;
        active_downloads.downloads.remove(model_name);
        if let Some(model) = models.get_mut(model_name) {
            if cancellation_won || result.is_err() {
                // A failed or cancelled download never removes a usable model that was already there.
                model.status = if existing_model_is_complete {
                    ModelStatus::Available
                } else {
                    ModelStatus::Missing
                };
            } else {
                model.status = ModelStatus::Available;
                model.path = model_dir.to_path_buf();
            }
        }
        active_downloads.revision = active_downloads.revision.wrapping_add(1);
        drop(models);
        drop(active_downloads);
        active_download.completion.send_replace(true);

        if cancellation_won {
            return Err(DownloadCancelled.into());
        }
        let final_progress = result?;
        if let Some(callback) = progress_callback {
            callback(final_progress);
        }
        log::info!("Download completed for Parakeet model: {}", model_name);
        Ok(())
    }

    /// Cancel an ongoing model download.
    pub async fn cancel_download(&self, model_name: &str) -> Result<CancelDownloadOutcome> {
        self.cancel_download_with_timeout(model_name, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT)
            .await
    }

    async fn cancel_download_with_timeout(
        &self,
        model_name: &str,
        cleanup_timeout: Duration,
    ) -> Result<CancelDownloadOutcome> {
        let active_download = {
            let active_downloads = self.active_downloads.lock().await;
            let Some(active_download) = active_downloads.downloads.get(model_name).cloned() else {
                return Ok(CancelDownloadOutcome::Cancelled);
            };
            active_download.cancellation.cancel();
            active_download
        };

        let mut completion = active_download.completion.subscribe();
        if !*completion.borrow() {
            match timeout(cleanup_timeout, completion.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    return Err(anyhow!(
                        "Download worker ended before completing cancellation cleanup"
                    ));
                }
                Err(_) => return Ok(CancelDownloadOutcome::Pending),
            }
        }

        Ok(CancelDownloadOutcome::Cancelled)
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crossbeam::queue::SegQueue;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    const TEST_MODEL_NAME: &str = "parakeet-test";
    const SMALL_MODEL_SPECS: &[ModelSpec] = &[ModelSpec {
        name: TEST_MODEL_NAME,
        size_mb: 1,
        quantization: QuantizationType::Int8,
        speed: "test",
        description: "test model",
    }];

    fn file(name: &str, content: &[u8]) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            remote_name: None,
            size: content.len() as u64,
            sha256: crate::model_download::sha256_hex(content),
        }
    }

    fn sources(base_url: &str) -> Vec<Source> {
        vec![Source { name: "test".to_string(), base_url: base_url.to_string() }]
    }

    fn small_artifacts() -> Vec<FileEntry> {
        vec![
            file("encoder.bin", b"ABCD"),
            file("decoder.bin", b"XYZ"),
            file("nemo.bin", b"NO"),
            file("vocab.txt", b"V"),
        ]
    }

    struct ExpectedResponse {
        filename: &'static str,
        range: Option<&'static str>,
        status: &'static str,
        content_length: Option<u64>,
        content_range: Option<&'static str>,
        body: &'static [u8],
        release_after_body: Option<oneshot::Receiver<()>>,
    }

    fn response(
        filename: &'static str,
        range: Option<&'static str>,
        status: &'static str,
        body: &'static [u8],
        content_range: Option<&'static str>,
    ) -> ExpectedResponse {
        ExpectedResponse {
            filename,
            range,
            status,
            content_length: Some(body.len() as u64),
            content_range,
            body,
            release_after_body: None,
        }
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let read = socket.read(&mut buffer).await.expect("read request");
            assert_ne!(read, 0, "request ended before its headers");
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                return String::from_utf8(request).expect("request is valid UTF-8");
            }
        }
    }

    async fn serve_requests(
        expected_responses: Vec<ExpectedResponse>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback test server");
        let address = listener.local_addr().expect("get loopback address");
        let server = tokio::spawn(async move {
            for expected in expected_responses {
                let (mut socket, _) = listener.accept().await.expect("accept test request");
                let request = read_request(&mut socket).await;
                assert!(
                    request.starts_with(&format!("GET /{} HTTP/", expected.filename)),
                    "unexpected request path: {request}"
                );
                let requested_range = request.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("range").then_some(value.trim())
                });
                assert_eq!(requested_range, expected.range);

                let mut headers = format!("HTTP/1.1 {}\r\nConnection: close\r\n", expected.status);
                if let Some(content_length) = expected.content_length {
                    headers.push_str(&format!("Content-Length: {content_length}\r\n"));
                }
                if let Some(content_range) = expected.content_range {
                    headers.push_str(&format!("Content-Range: {content_range}\r\n"));
                }
                headers.push_str("\r\n");
                socket
                    .write_all(headers.as_bytes())
                    .await
                    .expect("write response headers");
                socket
                    .write_all(expected.body)
                    .await
                    .expect("write response body");
                if let Some(release_after_body) = expected.release_after_body {
                    release_after_body.await.expect("release partial response");
                }
            }
        });

        (format!("http://{address}"), server)
    }

    async fn test_engine() -> (tempfile::TempDir, Arc<ParakeetEngine>, PathBuf) {
        let temp_dir = tempdir().expect("create temporary models directory");
        let engine = Arc::new(
            ParakeetEngine::new_with_models_dir(Some(temp_dir.path().to_path_buf()))
                .expect("create Parakeet engine"),
        );
        let model_dir = engine.models_dir.join(TEST_MODEL_NAME);
        engine.available_models.write().await.insert(
            TEST_MODEL_NAME.to_string(),
            ModelInfo {
                name: TEST_MODEL_NAME.to_string(),
                path: model_dir.clone(),
                size_mb: 1,
                quantization: QuantizationType::Int8,
                speed: "test".to_string(),
                status: ModelStatus::Missing,
                description: "test model".to_string(),
            },
        );
        (temp_dir, engine, model_dir)
    }

    async fn test_model_status(engine: &ParakeetEngine) -> ModelStatus {
        engine
            .available_models
            .read()
            .await
            .get(TEST_MODEL_NAME)
            .expect("test model remains registered")
            .status
            .clone()
    }

    #[tokio::test]
    async fn directory_validation_requires_exact_artifact_sizes() {
        let temp_dir = tempdir().expect("create temporary model directory");
        let small_artifacts = small_artifacts();
        for artifact in &small_artifacts {
            fs::write(
                temp_dir.path().join(&artifact.name),
                vec![0; artifact.size as usize],
            )
            .await
            .expect("seed exact artifact");
        }
        assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), &small_artifacts).is_ok());

        fs::remove_file(temp_dir.path().join("vocab.txt"))
            .await
            .expect("remove required artifact");
        assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), &small_artifacts).is_err());

        fs::write(temp_dir.path().join("vocab.txt"), [])
            .await
            .expect("restore undersized artifact");
        fs::write(temp_dir.path().join("encoder.bin"), [0; 3])
            .await
            .expect("seed one-byte-short artifact");
        assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), &small_artifacts).is_err());

        fs::write(temp_dir.path().join("encoder.bin"), [0; 5])
            .await
            .expect("seed one-byte-oversized artifact");
        assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), &small_artifacts).is_err());
    }

    #[tokio::test]
    async fn loading_releases_available_models_and_serializes_unload() {
        let (_temp_dir, engine, _model_dir) = test_engine().await;
        engine
            .available_models
            .write()
            .await
            .get_mut(TEST_MODEL_NAME)
            .expect("test model remains registered")
            .status = ModelStatus::Available;
        *engine.current_model_name.write().await = Some("previous-test-model".to_string());

        let hook = Arc::new(ModelLifecycleTestHook {
            load_started: tokio::sync::Notify::new(),
            continue_load: tokio::sync::Notify::new(),
            unload_attempted: tokio::sync::Notify::new(),
        });
        *engine.model_lifecycle_test_hook.lock().await = Some(Arc::clone(&hook));

        let load_engine = Arc::clone(&engine);
        let load = tokio::spawn(async move { load_engine.load_model(TEST_MODEL_NAME).await });
        tokio::time::timeout(Duration::from_secs(1), hook.load_started.notified())
            .await
            .expect("model load must reach its blocking task");

        {
            let mut models = tokio::time::timeout(
                Duration::from_secs(1),
                engine.available_models.write(),
            )
            .await
            .expect("model cache must remain writable during native loading");
            models
                .get_mut(TEST_MODEL_NAME)
                .expect("test model remains registered")
                .description = "updated while native load is paused".to_string();
        }

        let (unloaded_tx, mut unloaded_rx) = oneshot::channel();
        let unload_engine = Arc::clone(&engine);
        let unload = tokio::spawn(async move {
            let unloaded = unload_engine.unload_model().await;
            unloaded_tx
                .send(unloaded)
                .expect("unload receiver remains connected");
        });
        tokio::time::timeout(Duration::from_secs(1), hook.unload_attempted.notified())
            .await
            .expect("unload must reach the lifecycle boundary");
        assert!(matches!(
            unloaded_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        hook.continue_load.notify_one();
        tokio::time::timeout(Duration::from_secs(1), load)
            .await
            .expect("model load must finish after release")
            .expect("join model load task")
            .expect_err("empty model directory must fail loading");
        assert!(
            !tokio::time::timeout(Duration::from_secs(1), unloaded_rx)
                .await
                .expect("unload must finish after model load")
                .expect("unload sender remains connected")
        );
        tokio::time::timeout(Duration::from_secs(1), unload)
            .await
            .expect("unload task must join")
            .expect("join unload task");

        assert_eq!(
            engine
                .available_models
                .read()
                .await
                .get(TEST_MODEL_NAME)
                .expect("test model remains registered")
                .description,
            "updated while native load is paused"
        );
        assert!(engine.current_model.read().await.is_none());
        assert!(engine.current_model_name.read().await.is_none());
        *engine.model_lifecycle_test_hook.lock().await = None;
    }

    #[tokio::test]
    async fn completed_sibling_survives_403_then_retry_resumes_partial() {
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        fs::write(model_dir.join("encoder.bin"), b"ABCD")
            .await
            .expect("seed completed sibling");
        fs::write(model_dir.join("decoder.bin.part"), b"X")
            .await
            .expect("seed resumable artifact");

        let (base_url, server) = serve_requests(vec![response(
            "decoder.bin",
            Some("bytes=1-"),
            "403 Forbidden",
            b"",
            None,
        )])
        .await;
        let error = engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &sources(&base_url),
                &small_artifacts(),
                None,
            )
            .await
            .expect_err("403 must fail without destructive cleanup");
        server.await.expect("join 403 server");
        assert!(error.to_string().contains("403"));
        assert_eq!(fs::read(model_dir.join("encoder.bin")).await.unwrap(), b"ABCD");
        assert_eq!(fs::read(model_dir.join("decoder.bin.part")).await.unwrap(), b"X");
        assert!(!model_dir.join("decoder.bin").exists());
        assert!(!engine.active_downloads.lock().await.downloads.contains_key(TEST_MODEL_NAME));

        let (base_url, server) = serve_requests(vec![
            response(
                "decoder.bin",
                Some("bytes=1-"),
                "206 Partial Content",
                b"YZ",
                Some("bytes 1-2/3"),
            ),
            response("nemo.bin", None, "200 OK", b"NO", None),
            response("vocab.txt", None, "200 OK", b"V", None),
        ])
        .await;
        engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &sources(&base_url),
                &small_artifacts(),
                None,
            )
            .await
            .expect("retry resumes only the partial artifact");
        server.await.expect("join retry server");

        assert_eq!(fs::read(model_dir.join("encoder.bin")).await.unwrap(), b"ABCD");
        assert_eq!(fs::read(model_dir.join("decoder.bin")).await.unwrap(), b"XYZ");
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Available));
        assert!(!engine.active_downloads.lock().await.downloads.contains_key(TEST_MODEL_NAME));
    }

    #[tokio::test]
    async fn cancelled_near_complete_artifact_is_resumed_on_retry() {
        const PREFIX: &[u8] = &[b'A'; 99];
        let artifacts = vec![file("near.bin", &[PREFIX, b"B"].concat())];

        let (_temp_dir, engine, model_dir) = test_engine().await;
        let (release_tx, release_rx) = oneshot::channel();
        let (progress_tx, progress_rx) = oneshot::channel();
        let progress_tx = Arc::new(std::sync::Mutex::new(Some(progress_tx)));
        let (base_url, server) = serve_requests(vec![ExpectedResponse {
            filename: "near.bin",
            range: None,
            status: "200 OK",
            content_length: Some(100),
            content_range: None,
            body: PREFIX,
            release_after_body: Some(release_rx),
        }])
        .await;

        let download_engine = Arc::clone(&engine);
        let download_dir = model_dir.clone();
        let download_artifacts = artifacts.clone();
        let download = tokio::spawn(async move {
            download_engine
                .download_model_detailed_from_source(
                    TEST_MODEL_NAME,
                    &download_dir,
                    &sources(&base_url),
                    &download_artifacts,
                    Some(Box::new(move |progress| {
                        if progress.downloaded_bytes == 99 {
                            if let Some(sender) = progress_tx
                                .lock()
                                .expect("lock progress sender")
                                .take()
                            {
                                let _ = sender.send(());
                            }
                        }
                    })),
                )
                .await
        });

        tokio::time::timeout(Duration::from_secs(5), progress_rx)
            .await
            .expect("receive near-complete progress")
            .expect("progress sender remains connected");
        assert_eq!(
            engine
                .cancel_download_with_timeout(TEST_MODEL_NAME, Duration::from_secs(5))
                .await
                .expect("cancel near-complete download"),
            CancelDownloadOutcome::Cancelled
        );
        release_tx.send(()).expect("release partial response");
        let error = download
            .await
            .expect("join cancelled download")
            .expect_err("cancelled download must not succeed");
        server.await.expect("join partial-response server");

        assert!(is_download_cancelled(&error));
        assert_eq!(fs::metadata(model_dir.join("near.bin.part")).await.unwrap().len(), 99);
        assert!(!model_dir.join("near.bin").exists(), "a cancelled download is never activated");
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Missing));

        let (base_url, server) = serve_requests(vec![response(
            "near.bin",
            Some("bytes=99-"),
            "206 Partial Content",
            b"B",
            Some("bytes 99-99/100"),
        )])
        .await;
        engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &sources(&base_url),
                &artifacts,
                None,
            )
            .await
            .expect("retry resumes the cancelled near-complete artifact");
        server.await.expect("join retry server");

        assert_eq!(fs::metadata(model_dir.join("near.bin")).await.unwrap().len(), 100);
        assert!(!model_dir.join("near.bin.part").exists());
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Available));
    }

    #[tokio::test]
    async fn range_ignored_replaces_partial_with_honest_progress() {
        let artifacts = vec![file("model.bin", b"ABCD")];
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        fs::write(model_dir.join("model.bin.part"), b"zz")
            .await
            .expect("seed partial artifact");
        let events = Arc::new(SegQueue::new());
        let callback_events = Arc::clone(&events);

        let (base_url, server) = serve_requests(vec![response(
            "model.bin",
            Some("bytes=2-"),
            "200 OK",
            b"ABCD",
            None,
        )])
        .await;
        engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &sources(&base_url),
                &artifacts,
                Some(Box::new(move |progress| {
                    callback_events.push(progress);
                })),
            )
            .await
            .expect("range-ignored response replaces the partial artifact");
        server.await.expect("join range-ignored server");

        let events: Vec<_> = std::iter::from_fn(|| events.pop()).collect();
        assert_eq!(fs::read(model_dir.join("model.bin")).await.unwrap(), b"ABCD");
        assert!(events.iter().all(|progress| progress.downloaded_bytes <= progress.total_bytes));
        assert_eq!(events.last().expect("final event").percent, 100);
        assert!(events[..events.len() - 1].iter().all(|progress| progress.percent < 100));
    }

    #[tokio::test]
    async fn range_416_retries_fresh_with_honest_progress() {
        let artifacts = vec![file("model.bin", b"ABCD")];
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        fs::write(model_dir.join("model.bin.part"), b"zz")
            .await
            .expect("seed partial artifact");
        let events = Arc::new(SegQueue::new());
        let callback_events = Arc::clone(&events);

        let (base_url, server) = serve_requests(vec![
            response(
                "model.bin",
                Some("bytes=2-"),
                "416 Range Not Satisfiable",
                b"",
                Some("bytes */4"),
            ),
            response("model.bin", None, "200 OK", b"ABCD", None),
        ])
        .await;
        engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &sources(&base_url),
                &artifacts,
                Some(Box::new(move |progress| {
                    callback_events.push(progress);
                })),
            )
            .await
            .expect("416 should retry without Range");
        server.await.expect("join 416 server");

        assert_eq!(fs::read(model_dir.join("model.bin")).await.unwrap(), b"ABCD");
        assert!(std::iter::from_fn(|| events.pop())
            .all(|progress| progress.downloaded_bytes <= progress.total_bytes));
    }

    #[tokio::test]
    async fn invalid_or_short_response_never_publishes_available() {
        let artifacts = vec![file("complete.bin", b"C"), file("target.bin", b"ZXYZ")];
        let cases = vec![
            (
                "malformed",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "206 Partial Content",
                    content_length: Some(3),
                    content_range: Some("bytes invalid"),
                    body: b"XYZ",
                    release_after_body: None,
                },
            ),
            (
                "wrong-start",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "206 Partial Content",
                    content_length: Some(4),
                    content_range: Some("bytes 0-3/4"),
                    body: b"ABCD",
                    release_after_body: None,
                },
            ),
            (
                "wrong-total",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "206 Partial Content",
                    content_length: Some(3),
                    content_range: Some("bytes 1-3/5"),
                    body: b"XYZ",
                    release_after_body: None,
                },
            ),
            (
                "short",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "200 OK",
                    content_length: Some(4),
                    content_range: None,
                    body: b"ABC",
                    release_after_body: None,
                },
            ),
            (
                "overlong",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "200 OK",
                    content_length: Some(5),
                    content_range: None,
                    body: b"ABCDE",
                    release_after_body: None,
                },
            ),
        ];

        for (case_name, invalid_response) in cases {
            let (_temp_dir, engine, model_dir) = test_engine().await;
            fs::create_dir_all(&model_dir).await.expect("create model directory");
            fs::write(model_dir.join("complete.bin"), b"C")
                .await
                .expect("seed completed sibling");
            fs::write(model_dir.join("target.bin.part"), b"Z")
                .await
                .expect("seed retained prefix");

            let (base_url, server) = serve_requests(vec![invalid_response]).await;
            assert!(
                engine
                    .download_model_detailed_from_source(
                        TEST_MODEL_NAME,
                        &model_dir,
                        &sources(&base_url),
                        &artifacts,
                        None,
                    )
                    .await
                    .is_err(),
                "{case_name} response must fail"
            );
            server.await.expect("join invalid-response server");
            assert_eq!(fs::read(model_dir.join("complete.bin")).await.unwrap(), b"C");
            assert!(!matches!(test_model_status(&engine).await, ModelStatus::Available));
            assert!(!engine.active_downloads.lock().await.downloads.contains_key(TEST_MODEL_NAME));
        }
    }

    #[tokio::test]
    async fn pending_cancellation_keeps_owner_and_blocks_retry() {
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        fs::write(model_dir.join("encoder.bin"), b"AB")
            .await
            .expect("seed resumable prefix");
        let owner = engine
            .reserve_active_download(TEST_MODEL_NAME)
            .await
            .expect("reserve initial owner");

        assert_eq!(
            engine
                .cancel_download_with_timeout(TEST_MODEL_NAME, Duration::from_millis(1))
                .await
                .expect("request cancellation"),
            CancelDownloadOutcome::Pending
        );
        assert!(engine.reserve_active_download(TEST_MODEL_NAME).await.is_err());

        let error = engine
            .finish_download(
                TEST_MODEL_NAME,
                &model_dir,
                &small_artifacts(),
                &owner,
                Err(DownloadCancelled.into()),
                None,
            )
            .await
            .expect_err("cancelled owner must finish as cancellation");
        assert!(is_download_cancelled(&error));
        assert_eq!(fs::read(model_dir.join("encoder.bin")).await.unwrap(), b"AB");
        assert!(!engine.active_downloads.lock().await.downloads.contains_key(TEST_MODEL_NAME));
        assert!(engine.reserve_active_download(TEST_MODEL_NAME).await.is_ok());
    }

    #[tokio::test]
    async fn cancellation_wins_before_terminal_commit() {
        let artifacts = vec![file("model.bin", b"ABCD")];
        let (_temp_dir, engine, model_dir) = test_engine().await;
        let hook = Arc::new(DownloadStateTestHook {
            finalization_ready: tokio::sync::Notify::new(),
            continue_finalization: tokio::sync::Notify::new(),
            discovery_scanned: tokio::sync::Notify::new(),
            continue_discovery: tokio::sync::Notify::new(),
        });
        *engine.download_state_test_hook.lock().await = Some(Arc::clone(&hook));
        let events = Arc::new(SegQueue::new());
        let callback_events = Arc::clone(&events);

        let (base_url, server) =
            serve_requests(vec![response("model.bin", None, "200 OK", b"ABCD", None)]).await;
        let download_engine = Arc::clone(&engine);
        let download_dir = model_dir.clone();
        let download_artifacts = artifacts.clone();
        let download = tokio::spawn(async move {
            download_engine
                .download_model_detailed_from_source(
                    TEST_MODEL_NAME,
                    &download_dir,
                    &sources(&base_url),
                    &download_artifacts,
                    Some(Box::new(move |progress| {
                        callback_events.push(progress);
                    })),
                )
                .await
        });

        hook.finalization_ready.notified().await;
        assert_eq!(
            engine
                .cancel_download_with_timeout(TEST_MODEL_NAME, Duration::from_millis(1))
                .await
                .expect("request cancellation while finalization is paused"),
            CancelDownloadOutcome::Pending
        );
        hook.continue_finalization.notify_one();

        let error = download
            .await
            .expect("join download task")
            .expect_err("cancellation must win before terminal commit");
        server.await.expect("join cancellation server");
        assert!(is_download_cancelled(&error));
        assert!(std::iter::from_fn(|| events.pop()).all(|progress| progress.percent < 100));
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Missing));
        assert!(!engine.active_downloads.lock().await.downloads.contains_key(TEST_MODEL_NAME));
    }

    #[tokio::test]
    async fn changed_byte_is_rejected_and_the_existing_model_stays_available() {
        let artifacts = vec![file("model.bin", b"ABCD")];
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        // Same size, different content: usable by size, not the pinned file.
        fs::write(model_dir.join("model.bin"), b"OLD!").await.expect("seed existing model");

        let (base_url, server) =
            serve_requests(vec![response("model.bin", None, "200 OK", b"ABCE", None)]).await;
        let error = engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &sources(&base_url),
                &artifacts,
                None,
            )
            .await
            .expect_err("a changed byte must be rejected");
        server.await.expect("join tampered server");

        assert!(error.to_string().contains("model.bin"), "{error}");
        assert!(error.to_string().contains("SHA-256") && error.to_string().contains("slettet"), "{error}");
        assert_eq!(fs::read(model_dir.join("model.bin")).await.unwrap(), b"OLD!");
        assert!(!model_dir.join("model.bin.part").exists(), "the rejected download is deleted");
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Available));
    }

    #[tokio::test]
    async fn loading_refuses_an_installed_model_that_does_not_match_the_manifest() {
        let name = "parakeet-tdt-0.6b-v3-int8";
        let (_temp_dir, engine, _model_dir) = test_engine().await;
        let dir = engine.models_dir.join(name);
        fs::create_dir_all(&dir).await.expect("create model directory");
        for file in &model_download::entry("parakeet", name).expect("manifest entry").files {
            fs::write(dir.join(&file.name), b"not the pinned file").await.expect("seed file");
        }
        engine.available_models.write().await.insert(
            name.to_string(),
            ModelInfo {
                name: name.to_string(),
                path: dir,
                size_mb: 670,
                quantization: QuantizationType::Int8,
                speed: "test".to_string(),
                status: ModelStatus::Available,
                description: "test".to_string(),
            },
        );

        let error = engine.load_model(name).await.expect_err("a changed model must not load");
        assert!(error.to_string().contains("SHA-256"), "{error}");
        assert!(engine.current_model_name.read().await.is_none());
        assert!(matches!(
            engine.available_models.read().await.get(name).unwrap().status,
            ModelStatus::Corrupted { .. }
        ));
    }

    #[tokio::test]
    async fn discovery_retries_when_download_finalizes_after_disk_scan() {
        let (_temp_dir, engine, model_dir) = test_engine().await;
        let hook = Arc::new(DownloadStateTestHook {
            finalization_ready: tokio::sync::Notify::new(),
            continue_finalization: tokio::sync::Notify::new(),
            discovery_scanned: tokio::sync::Notify::new(),
            continue_discovery: tokio::sync::Notify::new(),
        });
        *engine.download_state_test_hook.lock().await = Some(Arc::clone(&hook));

        let discovery_engine = Arc::clone(&engine);
        let discovery = tokio::spawn(async move {
            discovery_engine
                .discover_models_from_specs(SMALL_MODEL_SPECS, &|_| small_artifacts())
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), hook.discovery_scanned.notified())
            .await
            .expect("discovery must finish its first disk scan");

        fs::create_dir_all(&model_dir).await.expect("create model directory");
        for artifact in &small_artifacts() {
            fs::write(
                model_dir.join(&artifact.name),
                vec![0; artifact.size as usize],
            )
            .await
            .expect("seed exact artifact");
        }

        let owner = engine
            .reserve_active_download(TEST_MODEL_NAME)
            .await
            .expect("reserve download owner");
        let finalization_engine = Arc::clone(&engine);
        let finalization_dir = model_dir.clone();
        let finalization_owner = Arc::clone(&owner);
        let finalization = tokio::spawn(async move {
            finalization_engine
                .finish_download(
                    TEST_MODEL_NAME,
                    &finalization_dir,
                    &small_artifacts(),
                    &finalization_owner,
                    Ok(DownloadProgress::new(10, 10, 0.0)),
                    None,
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), hook.finalization_ready.notified())
            .await
            .expect("finalization must reach its commit barrier");
        hook.continue_finalization.notify_one();
        tokio::time::timeout(Duration::from_secs(1), finalization)
            .await
            .expect("finalization must complete")
            .expect("join finalization task")
            .expect("successful download must finalize");

        *engine.download_state_test_hook.lock().await = None;
        hook.continue_discovery.notify_one();
        let discovered = tokio::time::timeout(Duration::from_secs(1), discovery)
            .await
            .expect("discovery must complete")
            .expect("join discovery task")
            .expect("discovery must succeed");

        let discovered_model = discovered
            .iter()
            .find(|model| model.name == TEST_MODEL_NAME)
            .expect("test model must be discovered");
        assert!(matches!(discovered_model.status, ModelStatus::Available));
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Available));
        assert!(!engine.active_downloads.lock().await.downloads.contains_key(TEST_MODEL_NAME));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_inference_is_not_serialized_on_the_engine() {
        use std::sync::atomic::AtomicUsize;

        let (_temp_dir, engine, _model_dir) = test_engine().await;
        let loads = Arc::new(AtomicUsize::new(0));
        engine
            .set_test_pool(TEST_MODEL_NAME, pool::tests::fake_pool(loads))
            .await;

        let long = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move { engine.transcribe_audio(vec![800.0]).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Status checks and a short call answer while the long call runs.
        let started = Instant::now();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), engine.is_model_loaded())
                .await
                .expect("status check must not wait for inference")
        );
        let short = tokio::time::timeout(Duration::from_millis(400), engine.transcribe_audio(vec![10.0]))
            .await
            .expect("short call must not wait for the long call")
            .expect("short call succeeds");
        assert_eq!(short, "10 ms");
        assert!(started.elapsed() < Duration::from_millis(400));
        assert!(!long.is_finished());

        assert_eq!(long.await.expect("join long call").expect("long call succeeds"), "800 ms");
        let (_, busy) = engine.usage().await.expect("model loaded");
        assert_eq!(busy, 0);
    }
}
