//! Optional start and stop sounds for dictation.
//!
//! The two sounds are short tones made here (a rising and a falling pair of
//! notes), so no third-party audio is involved. They play on the default
//! output device at the volume from the settings, and only for a dictation
//! that became ready (see `logic::startup`).

use std::f32::consts::PI;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Start,
    Stop,
}

const NOTE_MS: u32 = 70;

/// Mono samples at `rate` for `sound`, peak amplitude `volume` percent of a
/// moderate level (never louder than 0.5 of full scale).
pub fn tone(sound: Sound, volume: u32, rate: u32) -> Vec<f32> {
    let (first, second) = match sound {
        Sound::Start => (660.0, 880.0),
        Sound::Stop => (880.0, 660.0),
    };
    let amplitude = 0.5 * volume.min(100) as f32 / 100.0;
    let note_len = (rate * NOTE_MS / 1000) as usize;
    let mut out = Vec::with_capacity(note_len * 2);
    for freq in [first, second] {
        for i in 0..note_len {
            // Sine with a raised-cosine envelope, so there is no click.
            let t = i as f32 / note_len as f32;
            let envelope = 0.5 - 0.5 * (2.0 * PI * t).cos();
            out.push((2.0 * PI * freq * i as f32 / rate as f32).sin() * envelope * amplitude);
        }
    }
    out
}

/// Play `sound` without waiting for it. Failures are logged and ignored.
pub fn play(sound: Sound, volume: u32) {
    let spawned = std::thread::Builder::new()
        .name("dictation-sound".into())
        .spawn(move || {
            if let Err(e) = play_blocking(sound, volume) {
                log::info!("Dictation sound not played: {e}");
            }
        });
    if let Err(e) = spawned {
        log::info!("Dictation sound thread not started: {e}");
    }
}

fn play_blocking(sound: Sound, volume: u32) -> Result<(), String> {
    let device = cpal::default_host().default_output_device().ok_or("no output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let format = config.sample_format();
    let samples = tone(sound, volume, rate);
    let length = Duration::from_millis(u64::from(NOTE_MS) * 2 + 60);
    let mut position = 0usize;
    let mut next = move || {
        let value = samples.get(position).copied().unwrap_or(0.0);
        position += 1;
        value
    };
    let config: cpal::StreamConfig = config.into();
    let on_error = |e: cpal::StreamError| log::info!("Dictation sound stream error: {e}");
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let v = next();
                    frame.iter_mut().for_each(|s| *s = v);
                }
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let v = (next() * i16::MAX as f32) as i16;
                    frame.iter_mut().for_each(|s| *s = v);
                }
            },
            on_error,
            None,
        ),
        other => return Err(format!("unsupported output format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    std::thread::sleep(length);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_are_short_quiet_and_start_and_end_silent() {
        for sound in [Sound::Start, Sound::Stop] {
            let samples = tone(sound, 100, 48_000);
            assert_eq!(samples.len(), 48_000 * 140 / 1000);
            assert!(samples.iter().all(|s| s.abs() <= 0.5));
            assert!(samples.iter().any(|s| s.abs() > 0.3));
            assert!(samples[0].abs() < 0.01 && samples[samples.len() - 1].abs() < 0.01);
        }
        assert_ne!(tone(Sound::Start, 100, 48_000), tone(Sound::Stop, 100, 48_000));
    }

    #[test]
    fn volume_scales_the_tone_and_zero_is_silent() {
        let loud = tone(Sound::Start, 100, 16_000).iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let soft = tone(Sound::Start, 25, 16_000).iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((soft * 4.0 - loud).abs() < 0.01);
        assert!(tone(Sound::Start, 0, 16_000).iter().all(|s| *s == 0.0));
    }
}
