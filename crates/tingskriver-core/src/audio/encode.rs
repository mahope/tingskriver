//! FFmpeg encoding of finished audio. Never runs during a recording: the
//! recording writes WAV tracks (`track_writer`) and these functions encode
//! them after stop (`finalize`).

#![deny(clippy::unwrap_used, clippy::expect_used)]

use super::ffmpeg::find_ffmpeg_path;
use super::AudioDevice;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};
use tracing::{debug, error};

pub struct AudioInput {
    pub data: Arc<Vec<f32>>,
    pub sample_rate: u32,
    pub channels: u16,
    pub device: Arc<AudioDevice>,
}

/// Encode raw f32 samples to AAC in an mp4 file.
pub fn encode_single_audio(
    data: &[u8],
    sample_rate: u32,
    channels: u16,
    output_path: &PathBuf,
) -> anyhow::Result<()> {
    debug!("Starting FFmpeg process for {} bytes of audio data", data.len());

    if data.is_empty() {
        return Err(anyhow::anyhow!("No audio data provided for encoding"));
    }

    let ffmpeg_path = find_ffmpeg_path().ok_or_else(|| {
        anyhow::anyhow!("FFmpeg not found. Please install FFmpeg to save recordings.")
    })?;

    let mut done = false;
    run_ffmpeg_pipe(
        &ffmpeg_path,
        &["-f", "f32le", "-ar", &sample_rate.to_string(), "-ac", &channels.to_string()],
        "192k",
        output_path,
        |stdin| {
            if done {
                return Ok(false);
            }
            stdin.write_all(data)?;
            done = true;
            Ok(true)
        },
    )
}

/// Encode a stream of mono 16-bit samples to AAC in an mp4 container at
/// `output`. `next_block` fills the buffer with the next samples and returns
/// `false` at the end. FFmpeg's stderr is read on its own thread, so a chatty
/// FFmpeg cannot dead-lock the pipe.
pub fn encode_pcm_s16(
    ffmpeg: &Path,
    sample_rate: u32,
    bitrate: &str,
    output: &Path,
    mut next_block: impl FnMut(&mut Vec<i16>) -> std::io::Result<bool>,
) -> anyhow::Result<()> {
    let mut block = Vec::new();
    let mut bytes = Vec::new();
    run_ffmpeg_pipe(
        ffmpeg,
        &["-f", "s16le", "-ar", &sample_rate.to_string(), "-ac", "1"],
        bitrate,
        output,
        |stdin| {
            if !next_block(&mut block)? {
                return Ok(false);
            }
            bytes.clear();
            bytes.reserve(block.len() * 2);
            for s in &block {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            stdin.write_all(&bytes)?;
            Ok(true)
        },
    )
}

fn run_ffmpeg_pipe(
    ffmpeg: &Path,
    input_format: &[&str],
    bitrate: &str,
    output: &Path,
    mut feed: impl FnMut(&mut std::process::ChildStdin) -> std::io::Result<bool>,
) -> anyhow::Result<()> {
    let output_str = output
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Output path is not valid UTF-8: {}", output.display()))?;
    let mut command = Command::new(ffmpeg);
    command
        .args(["-nostats", "-loglevel", "error", "-y"])
        .args(input_format)
        .args(["-i", "pipe:0", "-c:a", "aac", "-b:a", bitrate, "-profile:a", "aac_low"])
        .args(["-movflags", "+faststart", "-f", "mp4", output_str])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    debug!("FFmpeg command: {:?}", command);
    let mut child = command
        .spawn()
        .map_err(|e| anyhow::anyhow!("Could not start FFmpeg ({}): {e}", ffmpeg.display()))?;
    let stderr_reader = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        })
    });

    let feed_result = match child.stdin.take() {
        Some(mut stdin) => {
            let mut result = Ok(());
            loop {
                match feed(&mut stdin) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            drop(stdin);
            result
        }
        None => Err(std::io::Error::new(std::io::ErrorKind::Other, "FFmpeg stdin unavailable")),
    };
    if feed_result.is_err() {
        // Input failed (or FFmpeg died): stop it rather than leave a half file.
        let _ = child.kill();
    }

    let status = child.wait().map_err(|e| anyhow::anyhow!("Waiting for FFmpeg failed: {e}"))?;
    let stderr = stderr_reader.and_then(|t| t.join().ok()).unwrap_or_default();
    if let Err(e) = feed_result {
        error!("Feeding FFmpeg failed: {e}; stderr: {stderr}");
        return Err(anyhow::anyhow!("Feeding FFmpeg failed: {e} {}", stderr.trim()));
    }
    if !status.success() {
        error!("FFmpeg failed with status {status}: {stderr}");
        return Err(anyhow::anyhow!("FFmpeg failed with status {status}: {}", stderr.trim()));
    }
    Ok(())
}
