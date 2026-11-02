#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendPreference {
    Auto,
    Silero,
    Energy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Silero,
    Energy,
}
#[derive(Debug, Clone)]
pub struct TrimConfig {
    pub threshold: f32,
    pub min_speech_ms: u32,
    pub min_silence_ms: u32,
    pub speech_pad_ms: u32,
    pub backend: BackendPreference,
}
impl Default for TrimConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_speech_ms: 150,
            min_silence_ms: 500,
            speech_pad_ms: 200,
            backend: BackendPreference::Auto,
        }
    }
}
impl TrimConfig {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.threshold.is_finite() && (0.0..=1.0).contains(&self.threshold),
            "VAD threshold must be between zero and one"
        );
        anyhow::ensure!(
            self.min_speech_ms > 0 && self.min_silence_ms > 0,
            "VAD minimum durations must be positive"
        );
        Ok(())
    }
}
/// The probe is injectable so missing-runtime behavior needs no environment changes.
/// An explicit Silero preference still falls back when runtime startup is unsafe.
pub fn select_backend(
    preference: BackendPreference,
    runtime_available: impl FnOnce() -> bool,
) -> Backend {
    if preference == BackendPreference::Energy {
        log::info!("VAD backend: energy (explicit preference)");
        Backend::Energy
    } else if runtime_available() {
        log::info!("VAD backend: Silero (runtime available)");
        Backend::Silero
    } else {
        log::info!("VAD backend: energy (ONNX Runtime unavailable)");
        Backend::Energy
    }
}
