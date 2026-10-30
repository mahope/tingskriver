//! Small audio helpers for dictation: downmix, resample, level and WAV.

pub const TARGET_RATE: u32 = 16_000;

/// Average interleaved channels into mono.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Linear-interpolation resampler. Good enough for speech recognition.
pub fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() || from_rate == 0 {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    // When downsampling, average over the source span to avoid aliasing.
    let span = ratio.floor().max(1.0) as usize;
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = pos.floor() as usize;
        if span > 1 {
            let end = (idx + span).min(input.len());
            let slice = &input[idx..end];
            out.push(slice.iter().sum::<f32>() / slice.len().max(1) as f32);
        } else {
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = *input.get(idx + 1).unwrap_or(&a);
            out.push(a + (b - a) * frac);
        }
    }
    out
}

/// Peak-ish level in 0.0..=1.0 for the overlay meter (RMS on a dB scale).
pub fn level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    if rms <= 0.0 {
        return 0.0;
    }
    // -60 dBFS → 0, 0 dBFS → 1.
    let db = 20.0 * rms.log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Encode mono f32 samples as a 16-bit PCM WAV file.
pub fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Duration in milliseconds of mono samples at `rate`.
pub fn duration_ms(samples: usize, rate: u32) -> u64 {
    if rate == 0 {
        return 0;
    }
    samples as u64 * 1000 / rate as u64
}

/// True if the recording has any audible speech-level signal at all.
pub fn has_signal(samples: &[f32]) -> bool {
    samples.chunks(1600).any(|chunk| level(chunk) > 0.25)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_stereo() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn resample_lengths() {
        let one_second_48k = vec![0.0f32; 48_000];
        assert_eq!(resample(&one_second_48k, 48_000, 16_000).len(), 16_000);
        let one_second_44k = vec![0.0f32; 44_100];
        assert_eq!(resample(&one_second_44k, 44_100, 16_000).len(), 16_000);
        let one_second_8k = vec![0.0f32; 8_000];
        assert_eq!(resample(&one_second_8k, 8_000, 16_000).len(), 16_000);
        assert!(resample(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn resample_keeps_dc_level() {
        let input = vec![0.5f32; 4800];
        let out = resample(&input, 48_000, 16_000);
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn level_scale() {
        assert_eq!(level(&[]), 0.0);
        assert_eq!(level(&[0.0; 100]), 0.0);
        assert!((level(&[1.0; 100]) - 1.0).abs() < 1e-6);
        let quiet = level(&[0.001; 100]);
        assert!(quiet < 0.1, "{quiet}");
        assert!(has_signal(&[0.3; 3200]));
        assert!(!has_signal(&[0.0005; 3200]));
    }

    #[test]
    fn wav_header() {
        let bytes = wav_bytes(&[0.0, 1.0, -1.0], 16_000);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 16_000);
        assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([bytes[48], bytes[49]]), -i16::MAX);
    }

    #[test]
    fn durations() {
        assert_eq!(duration_ms(16_000, 16_000), 1000);
        assert_eq!(duration_ms(8_000, 16_000), 500);
        assert_eq!(duration_ms(1, 0), 0);
    }
}
