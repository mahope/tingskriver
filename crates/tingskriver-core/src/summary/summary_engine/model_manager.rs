// Model manager for built-in AI models - handles downloads and lifecycle
// Follows the same pattern as whisper_engine/whisper_engine.rs for consistency

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::model_download::{self, ProgressSink, TransferError, TransferOptions};

use super::models::{get_available_models, get_model_by_name};

// ============================================================================
// Model Status Types
// ============================================================================

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
            ((downloaded as f64 / total as f64) * 100.0) as u8
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

/// Model status in the system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelStatus {
    /// Model is not yet downloaded
    NotDownloaded,

    /// Model is currently being downloaded (progress 0-100)
    Downloading { progress: u8 },

    /// Model is downloaded and ready to use
    Available,

    /// Model file is corrupted and needs redownload
    Corrupted { file_size: u64, expected_min_size: u64 },

    /// Error occurred with the model
    Error(String),
}

/// Model information for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model name (e.g., "gemma3:1b")
    pub name: String,

    /// Display name for UI
    pub display_name: String,

    /// Current status
    pub status: ModelStatus,

    /// File path (if available)
    pub path: PathBuf,

    /// Size in MB
    pub size_mb: u64,

    /// Context window size in tokens
    pub context_size: u32,

    /// Description
    pub description: String,

    /// GGUF filename on disk
    pub gguf_file: String,
}

// ============================================================================
// Model Manager
// ============================================================================

pub struct ModelManager {
    /// Directory where models are stored
    models_dir: PathBuf,

    /// Currently available models with their status
    available_models: Arc<RwLock<HashMap<String, ModelInfo>>>,

    /// Active downloads (model names)
    active_downloads: Arc<RwLock<HashSet<String>>>,

    /// Cancellation flag for current download
    cancel_download_flag: Arc<RwLock<Option<String>>>,

    /// Cancellation tokens of the running downloads
    cancel_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

/// Turns byte counts from the shared downloader into progress callbacks and model status.
struct SummaryProgress {
    models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    model_name: String,
    callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    total_bytes: u64,
    last_file_bytes: u64,
    bytes_since_report: u64,
    last_percent: u8,
    last_report: std::time::Instant,
    first: bool,
}

#[async_trait]
impl ProgressSink for SummaryProgress {
    async fn bytes(&mut self, file_bytes: u64) {
        self.bytes_since_report = self
            .bytes_since_report
            .saturating_add(file_bytes.saturating_sub(self.last_file_bytes));
        self.last_file_bytes = file_bytes;
        let percent = if self.total_bytes > 0 {
            ((file_bytes as f64 / self.total_bytes as f64) * 100.0).min(99.0) as u8
        } else {
            0
        };
        let elapsed = self.last_report.elapsed();
        if !(self.first || percent > self.last_percent || elapsed.as_millis() >= 500) {
            return;
        }
        let speed_mbps = if !self.first && elapsed.as_secs_f64() > 0.0 {
            (self.bytes_since_report as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64()
        } else {
            0.0
        };
        if let Some(callback) = &self.callback {
            callback(DownloadProgress::new(
                file_bytes,
                self.total_bytes,
                speed_mbps,
            ));
        }
        if let Some(model_info) = self.models.write().await.get_mut(&self.model_name) {
            model_info.status = ModelStatus::Downloading { progress: percent };
        }
        self.first = false;
        self.last_percent = percent;
        self.last_report = std::time::Instant::now();
        self.bytes_since_report = 0;
    }
}

impl ModelManager {
    /// Create a new model manager with default models directory
    pub fn new() -> Result<Self> {
        Self::new_with_models_dir(None)
    }

    /// Create a new model manager with custom models directory
    pub fn new_with_models_dir(models_dir: Option<PathBuf>) -> Result<Self> {
        let models_dir = if let Some(dir) = models_dir {
            dir
        } else {
            // Fallback: Use current directory in development
            let current_dir = std::env::current_dir()
                .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;

            if cfg!(debug_assertions) {
                // Development mode
                current_dir.join("models").join("summary")
            } else {
                // Production mode fallback (caller should provide path)
                log::warn!("ModelManager: No models directory provided, using fallback path");
                dirs::data_dir()
                    .or_else(|| dirs::home_dir())
                    .ok_or_else(|| anyhow!("Could not find system data directory"))?
                    .join("Meetily")
                    .join("models")
                    .join("summary")
            }
        };

        log::info!(
            "Built-in AI ModelManager using directory: {}",
            models_dir.display()
        );

        Ok(Self {
            models_dir,
            available_models: Arc::new(RwLock::new(HashMap::new())),
            active_downloads: Arc::new(RwLock::new(HashSet::new())),
            cancel_download_flag: Arc::new(RwLock::new(None)),
            cancel_tokens: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Initialize and scan for existing models
    pub async fn init(&self) -> Result<()> {
        // Create models directory if it doesn't exist
        if !self.models_dir.exists() {
            fs::create_dir_all(&self.models_dir).await?;
            log::info!("Created models directory: {}", self.models_dir.display());
        }

        // Scan for existing models
        self.scan_models().await?;

        Ok(())
    }

    /// Scan models directory and update status
    pub async fn scan_models(&self) -> Result<()> {
        let start = std::time::Instant::now();

        log::info!(
            "Starting model scan in directory: {}",
            self.models_dir.display()
        );

        let model_defs = get_available_models();
        let mut models_map = HashMap::new();

        for model_def in model_defs {
            let model_path = self.models_dir.join(&model_def.gguf_file);
            log::debug!(
                "Checking model '{}' at path: {}",
                model_def.name,
                model_path.display()
            );

            let is_actively_downloading = {
                let active = self.active_downloads.read().await;
                active.contains(&model_def.name)
            };

            // If actively downloading, preserve existing status from memory
            if is_actively_downloading {
                let existing_info = {
                    let models = self.available_models.read().await;
                    models.get(&model_def.name).cloned()
                };

                if let Some(info) = existing_info {
                    // Preserve existing status (should be Downloading)
                    models_map.insert(model_def.name.clone(), info);
                    log::debug!(
                        "Model '{}': Preserving Downloading status during scan",
                        model_def.name
                    );
                    continue;
                }
            }

            let status = if model_path.exists() {
                // Check if file size matches expected size (basic validation)
                match fs::metadata(&model_path).await {
                    Ok(metadata) => {
                        let file_size_mb = metadata.len() / (1024 * 1024);

                        // Allow 10% variance for file size check
                        let expected_min = (model_def.size_mb as f64 * 0.9) as u64;
                        let expected_max = (model_def.size_mb as f64 * 1.1) as u64;

                        log::info!(
                            "Model '{}': found {} MB (expected {}-{} MB)",
                            model_def.name,
                            file_size_mb,
                            expected_min,
                            expected_max
                        );

                        if file_size_mb >= expected_min && file_size_mb <= expected_max {
                            log::info!("Model '{}': AVAILABLE", model_def.name);
                            ModelStatus::Available
                        } else {
                            log::warn!(
                                "Model '{}': CORRUPTED (size mismatch: {} MB, expected {} MB)",
                                model_def.name,
                                file_size_mb,
                                model_def.size_mb
                            );
                            ModelStatus::Corrupted {
                                file_size: file_size_mb,
                                expected_min_size: expected_min,
                            }
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Model '{}': Failed to read metadata: {}",
                            model_def.name,
                            e
                        );
                        ModelStatus::Error(format!("Failed to read metadata: {}", e))
                    }
                }
            } else {
                log::debug!("Model '{}': NOT FOUND", model_def.name);
                ModelStatus::NotDownloaded
            };

            let model_info = ModelInfo {
                name: model_def.name.clone(),
                display_name: model_def.display_name.clone(),
                status,
                path: model_path,
                size_mb: model_def.size_mb,
                context_size: model_def.context_size,
                description: model_def.description.clone(),
                gguf_file: model_def.gguf_file.clone(),
            };

            models_map.insert(model_def.name.clone(), model_info);
        }

        let model_count = models_map.len();

        let mut models = self.available_models.write().await;
        *models = models_map;

        let elapsed = start.elapsed();
        log::info!(
            "Model scan complete: {} models checked in {:?}",
            model_count,
            elapsed
        );
        Ok(())
    }

    /// Get list of all models with their status
    pub async fn list_models(&self) -> Vec<ModelInfo> {
        self.available_models
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    /// Get info for a specific model
    pub async fn get_model_info(&self, model_name: &str) -> Option<ModelInfo> {
        self.available_models
            .read()
            .await
            .get(model_name)
            .cloned()
    }

    /// Check if a model is ready to use
    /// If refresh=true, scans filesystem before checking (slower but accurate)
    pub async fn is_model_ready(&self, model_name: &str, refresh: bool) -> bool {
        if refresh {
            if let Err(e) = self.scan_models().await {
                log::error!("Failed to scan models: {}", e);
                return false;
            }
        }

        let Some(info) = self.get_model_info(model_name).await else {
            return false;
        };
        if info.status != ModelStatus::Available {
            return false;
        }
        // A file that no longer matches the pinned SHA-256 is never used.
        if let Some(entry) = model_download::entry("summary", model_name) {
            let dir = info.path.parent().unwrap_or(&self.models_dir).to_path_buf();
            if let Err(error) =
                model_download::verify_installed(&dir, &entry.files[0], &CancellationToken::new())
                    .await
            {
                log::error!(
                    "Built-in model {} failed verification: {}",
                    model_name,
                    error
                );
                if matches!(error, TransferError::InstalledMismatch { .. }) {
                    if let Some(model_info) =
                        self.available_models.write().await.get_mut(model_name)
                    {
                        model_info.status = ModelStatus::Corrupted {
                            file_size: info
                                .path
                                .metadata()
                                .map(|m| m.len() / (1024 * 1024))
                                .unwrap_or(0),
                            expected_min_size: entry.files[0].size / (1024 * 1024),
                        };
                    }
                }
                return false;
            }
        }
        true
    }

    /// Download a model with simple percentage callback (backward compatible)
    pub async fn download_model(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
    ) -> Result<()> {
        // Wrap the simple callback to use detailed progress internally
        let detailed_callback: Option<Box<dyn Fn(DownloadProgress) + Send>> =
            progress_callback.map(|cb| {
                Box::new(move |p: DownloadProgress| cb(p.percent)) as Box<dyn Fn(DownloadProgress) + Send>
            });
        self.download_model_detailed(model_name, detailed_callback).await
    }

    /// Download a model with detailed progress (MB, speed, etc.)
    pub async fn download_model_detailed(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        log::info!("Starting download for model: {}", model_name);

        // Check if already downloading
        {
            let active = self.active_downloads.read().await;
            if active.contains(model_name) {
                log::warn!("Download already in progress for model: {}", model_name);
                return Err(anyhow!("Download already in progress"));
            }
        }

        // Get model definition
        let model_def = get_model_by_name(model_name)
            .ok_or_else(|| anyhow!("Unknown model: {}", model_name))?;

        // Add to active downloads
        {
            let mut active = self.active_downloads.write().await;
            active.insert(model_name.to_string());
        }

        // Clear cancellation flag
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            *cancel_flag = None;
        }

        // Update status to downloading
        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::Downloading { progress: 0 };
            }
        }

        let file_path = self.models_dir.join(&model_def.gguf_file);

        // Pinned size, SHA-256 and sources (main source first, then the reserve mirror).
        let Some(entry) = model_download::entry("summary", model_name) else {
            self.finish_failed_download(
                model_name,
                &file_path,
                "Model is missing from the model manifest".to_string(),
            )
            .await;
            return Err(anyhow!(
                "Model {} is missing from the model manifest",
                model_name
            ));
        };
        let file = &entry.files[0];

        log::info!(
            "Downloading {} from the pinned sources ({} source(s))",
            model_name,
            entry.sources.len()
        );
        log::info!("Saving to: {}", file_path.display());

        let cancel = CancellationToken::new();
        self.cancel_tokens
            .write()
            .await
            .insert(model_name.to_string(), cancel.clone());

        let result = async {
            let client =
                model_download::client().map_err(|e| TransferError::Failed(e.to_string()))?;
            let mut sink = SummaryProgress {
                models: Arc::clone(&self.available_models),
                model_name: model_name.to_string(),
                callback: progress_callback,
                total_bytes: file.size,
                last_file_bytes: 0,
                bytes_since_report: 0,
                last_percent: 0,
                last_report: std::time::Instant::now(),
                first: true,
            };
            let fetched = model_download::fetch_file(
                &client,
                &self.models_dir,
                file,
                &entry.sources,
                &cancel,
                &TransferOptions::default(),
                &mut sink,
            )
            .await?;
            if let Some(callback) = &sink.callback {
                callback(DownloadProgress::new(file.size, file.size, 0.0));
            }
            Ok::<_, TransferError>(fetched)
        }
        .await;

        self.cancel_tokens.write().await.remove(model_name);

        match result {
            Ok(fetched) => {
                log::info!(
                    "Download completed for model {} (source: {})",
                    model_name,
                    fetched.source
                );

                if let Err(e) = self.validate_gguf_file(&file_path).await {
                    log::error!("Downloaded file failed validation: {}", e);
                    // Verified by SHA-256 but not a GGUF: never keep it as the active model.
                    let _ = fs::remove_file(&file_path).await;
                    model_download::remove_sidecars(&self.models_dir, &model_def.gguf_file).await;
                    self.finish_failed_download(
                        model_name,
                        &file_path,
                        format!("Validation failed: {}", e),
                    )
                    .await;
                    return Err(anyhow!("File validation failed: {}", e));
                }

                {
                    let mut models = self.available_models.write().await;
                    if let Some(model_info) = models.get_mut(model_name) {
                        model_info.status = ModelStatus::Available;
                        model_info.path = file_path.clone();
                    }
                }
                self.active_downloads.write().await.remove(model_name);
                Ok(())
            }
            Err(TransferError::Cancelled) => {
                log::info!("Download cancelled for model: {}", model_name);
                // The partial file is kept for resume; a model that was already installed stays usable.
                let status = if Self::is_installed_with_pinned_size(&file_path, file.size).await {
                    ModelStatus::Available
                } else {
                    ModelStatus::NotDownloaded
                };
                {
                    let mut models = self.available_models.write().await;
                    if let Some(model_info) = models.get_mut(model_name) {
                        model_info.status = status;
                    }
                }
                self.active_downloads.write().await.remove(model_name);
                // Special marker prefix to distinguish cancellation from other errors
                Err(anyhow!("CANCELLED: Download cancelled by user"))
            }
            Err(error) => {
                log::error!("Download failed for {}: {}", model_name, error);
                self.finish_failed_download(model_name, &file_path, error.to_string())
                    .await;
                Err(anyhow::Error::from(error))
            }
        }
    }

    async fn is_installed_with_pinned_size(path: &PathBuf, size: u64) -> bool {
        matches!(fs::metadata(path).await, Ok(metadata) if metadata.len() == size)
    }

    /// A failed download never removes a model that was already installed: it stays available,
    /// otherwise the model is marked with the error so the UI can offer a retry.
    async fn finish_failed_download(&self, model_name: &str, file_path: &PathBuf, message: String) {
        let pinned_size = model_download::entry("summary", model_name)
            .and_then(|entry| entry.files.first().map(|file| file.size));
        let usable = match pinned_size {
            Some(size) => Self::is_installed_with_pinned_size(file_path, size).await,
            None => false,
        };
        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = if usable { ModelStatus::Available } else { ModelStatus::Error(message) };
            }
        }
        self.active_downloads.write().await.remove(model_name);
    }

    /// Validate that a file is a valid GGUF model
    async fn validate_gguf_file(&self, path: &PathBuf) -> Result<()> {
        let mut file = fs::File::open(path).await?;

        // Read first 4 bytes to check for GGUF magic number
        use tokio::io::AsyncReadExt;
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic).await?;

        // GGUF magic number is "GGUF" (0x47475546)
        if &magic == b"GGUF" {
            Ok(())
        } else if &magic == b"ggjt" || &magic == b"ggla" || &magic == b"ggml" {
            // Older formats (GGML, GGJT)
            Ok(())
        } else {
            Err(anyhow!(
                "Invalid model file: magic number {:?} doesn't match GGUF/GGML",
                magic
            ))
        }
    }

    /// Cancel an ongoing download
    pub async fn cancel_download(&self, model_name: &str) -> Result<()> {
        log::info!("Cancelling download for model: {}", model_name);

        // Set cancellation flag - download loop will detect this and handle cleanup
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            *cancel_flag = Some(model_name.to_string());
        }
        if let Some(token) = self.cancel_tokens.read().await.get(model_name) {
            token.cancel();
        }

        // Note: active_downloads cleanup is handled by the download loop when it detects
        // the cancellation flag. This avoids double-removal race condition.

        // Update status immediately for UI responsiveness
        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::NotDownloaded;
            }
        }

        // Brief delay to let download loop detect cancellation
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(())
    }

    /// Delete a corrupted or available model file
    pub async fn delete_model(&self, model_name: &str) -> Result<()> {
        log::info!("Deleting model: {}", model_name);

        let model_def = get_model_by_name(model_name)
            .ok_or_else(|| anyhow!("Unknown model: {}", model_name))?;

        let file_path = self.models_dir.join(&model_def.gguf_file);

        if file_path.exists() {
            fs::remove_file(&file_path).await?;
            log::info!("Deleted model file: {}", file_path.display());
        }
        model_download::remove_sidecars(&self.models_dir, &model_def.gguf_file).await;

        // Update status
        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::NotDownloaded;
            }
        }

        Ok(())
    }

    /// Get models directory path
    pub fn get_models_directory(&self) -> PathBuf {
        self.models_dir.clone()
    }
}
