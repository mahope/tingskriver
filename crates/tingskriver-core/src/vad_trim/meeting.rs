use super::{select_backend, TrimConfig};
use crate::audio::vad::SpeechSegment;
use anyhow::Result;

/// Compatibility adapter for saved-meeting retranscription. The provider is
/// called per original segment by that path, so its timestamps stay original.
pub fn get_speech_chunks_with_progress(
    samples: &[f32],
    min_silence_ms: u32,
    mut progress: impl FnMut(u32, usize) -> bool,
) -> Result<Vec<SpeechSegment>> {
    let cfg = TrimConfig {
        min_silence_ms,
        ..Default::default()
    };
    let backend = select_backend(cfg.backend, || {
        crate::ensure_onnx_runtime_available().is_ok()
    });
    cfg.validate()?;
    let processor = super::trim::make_processor(backend, &cfg);
    let mut last_percent = None;
    let input = samples.iter().copied().enumerate().map(|(i, sample)| {
        if i % 16000 == 0 {
            let percent = ((i as f64 / samples.len().max(1) as f64) * 100.0) as u32;
            if last_percent.map_or(true, |last| percent >= last + 5) {
                last_percent = Some(percent);
                anyhow::ensure!(progress(percent, 0), "VAD processing cancelled");
            }
        }
        Ok(sample)
    });
    let ranges = super::trim::detect(input, &cfg, processor)?;
    anyhow::ensure!(progress(100, ranges.len()), "VAD processing cancelled");
    Ok(ranges
        .into_iter()
        .map(|(start, end)| SpeechSegment {
            samples: samples[start..end].to_vec(),
            start_timestamp_ms: start as f64 / 16.0,
            end_timestamp_ms: end as f64 / 16.0,
            // The energy backend does not provide a calibrated probability.
            confidence: 0.0,
        })
        .collect())
}
