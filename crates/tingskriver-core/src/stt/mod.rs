//! Native inference runs only in the `tingskriver-stt` executable.
mod client;
pub mod ipc;
mod server;

pub use client::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static WORKER: AtomicBool = AtomicBool::new(false);

pub fn is_worker() -> bool {
    WORKER.load(Ordering::Relaxed)
}

#[derive(Serialize, Deserialize)]
pub enum Request {
    Parakeet {
        path: PathBuf,
        quantized: bool,
    },
    Transcribe,
    Echo,
    Vad {
        rate: u32,
        config: VadSettings,
    },
    VadProcess {
        flush: bool,
    },
    VadBuffered,
    Whisper {
        directory: PathBuf,
        name: String,
    },
    WhisperTranscribe {
        language: Option<String>,
        confidence: bool,
    },
}

pub struct WhisperClient {
    init: Vec<u8>,
    slots: [std::sync::Mutex<Option<Client>>; 2],
}
impl WhisperClient {
    pub fn load(directory: PathBuf, name: String) -> Result<Self, String> {
        let init =
            serde_json::to_vec(&Request::Whisper { directory, name }).map_err(|e| e.to_string())?;
        let first = Client::new(init.clone())?;
        Ok(Self {
            init,
            slots: [
                std::sync::Mutex::new(Some(first)),
                std::sync::Mutex::new(None),
            ],
        })
    }
    pub fn transcribe(
        &self,
        samples: &[f32],
        language: Option<String>,
        confidence: bool,
    ) -> Result<(String, f32, bool), String> {
        let mut slot = if let Ok(slot) = self.slots[0].try_lock() {
            slot
        } else {
            self.slots[1].lock().map_err(|_| client::CRASH_ERROR)?
        };
        if slot.is_none() {
            *slot = Some(Client::new(self.init.clone())?);
        }
        let request = serde_json::to_vec(&Request::WhisperTranscribe {
            language,
            confidence,
        })
        .map_err(|e| e.to_string())?;
        let frame = slot.as_mut().unwrap().call(&request, samples)?;
        serde_json::from_slice::<Result<(String, f32, bool), String>>(&frame.control)
            .map_err(|_| client::CRASH_ERROR.to_string())?
    }
}

#[derive(Serialize, Deserialize)]
pub struct VadSettings {
    positive: f32,
    negative: f32,
    pre: std::time::Duration,
    post: std::time::Duration,
    redemption: std::time::Duration,
    minimum: std::time::Duration,
    sample_rate: usize,
}

impl From<silero_rs::VadConfig> for VadSettings {
    fn from(c: silero_rs::VadConfig) -> Self {
        Self {
            positive: c.positive_speech_threshold,
            negative: c.negative_speech_threshold,
            pre: c.pre_speech_pad,
            post: c.post_speech_pad,
            redemption: c.redemption_time,
            minimum: c.min_speech_time,
            sample_rate: c.sample_rate,
        }
    }
}
impl From<VadSettings> for silero_rs::VadConfig {
    fn from(c: VadSettings) -> Self {
        Self {
            positive_speech_threshold: c.positive,
            negative_speech_threshold: c.negative,
            pre_speech_pad: c.pre,
            post_speech_pad: c.post,
            redemption_time: c.redemption,
            min_speech_time: c.minimum,
            sample_rate: c.sample_rate,
        }
    }
}

pub struct VadClient {
    client: Client,
}
impl VadClient {
    pub fn new(rate: u32, config: silero_rs::VadConfig) -> Result<Self, String> {
        let init = serde_json::to_vec(&Request::Vad {
            rate,
            config: config.into(),
        })
        .map_err(|e| e.to_string())?;
        Ok(Self {
            client: Client::new(init)?,
        })
    }
    pub fn process(
        &mut self,
        samples: &[f32],
        flush: bool,
    ) -> Result<Vec<crate::audio::vad::SpeechSegment>, String> {
        let request =
            serde_json::to_vec(&Request::VadProcess { flush }).map_err(|e| e.to_string())?;
        let frame = self.client.call(&request, samples)?;
        let metadata =
            serde_json::from_slice::<Result<Vec<(usize, f64, f64, f32)>, String>>(&frame.control)
                .map_err(|_| client::CRASH_ERROR.to_string())??;
        let mut offset = 0usize;
        let mut segments = Vec::new();
        for (length, start_timestamp_ms, end_timestamp_ms, confidence) in metadata {
            let end = offset.checked_add(length).ok_or(client::CRASH_ERROR)?;
            let samples = frame
                .samples
                .get(offset..end)
                .ok_or(client::CRASH_ERROR)?
                .to_vec();
            segments.push(crate::audio::vad::SpeechSegment {
                samples,
                start_timestamp_ms,
                end_timestamp_ms,
                confidence,
            });
            offset = end;
        }
        if offset != frame.samples.len() {
            return Err(client::CRASH_ERROR.into());
        }
        Ok(segments)
    }
    #[cfg(test)]
    pub fn buffered(&mut self) -> Result<usize, String> {
        let frame = self
            .client
            .call(&serde_json::to_vec(&Request::VadBuffered).unwrap(), &[])?;
        serde_json::from_slice(&frame.control).map_err(|e| e.to_string())
    }
}

pub struct RemoteModel {
    client: Client,
}

impl RemoteModel {
    pub fn load(path: &std::path::Path, quantized: bool) -> Result<Self, String> {
        let init = serde_json::to_vec(&Request::Parakeet {
            path: path.into(),
            quantized,
        })
        .map_err(|e| e.to_string())?;
        Ok(Self {
            client: Client::new(init)?,
        })
    }
}

impl crate::parakeet_engine::pool::SpeechModel for RemoteModel {
    fn transcribe(&mut self, samples: Vec<f32>) -> Result<String, String> {
        let control = serde_json::to_vec(&Request::Transcribe).map_err(|e| e.to_string())?;
        let frame = self.client.call(&control, &samples)?;
        serde_json::from_slice::<Result<String, String>>(&frame.control)
            .map_err(|_| client::CRASH_ERROR.to_string())?
    }
}

pub fn run_worker() -> anyhow::Result<()> {
    WORKER.store(true, Ordering::Relaxed);
    server::run()
}
