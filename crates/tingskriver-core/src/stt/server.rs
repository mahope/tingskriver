use super::{ipc, Request};
use crate::parakeet_engine::pool::BoxedModel;
use std::io::{BufReader, BufWriter};

/// Test hook: `TINGSKRIVER_STT_FAULT=<abort|segv|hang>@<init|load|run>[:gpu]`.
/// `init` fires at process start, `load` on the first request (the model
/// load) and `run` on every later one. With `:gpu` only a worker started for
/// the GPU misbehaves, so a CPU retry can be shown to work. Debug builds only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum FaultKind {
    Abort,
    Segv,
    Hang,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum FaultPhase {
    Init,
    Load,
    Run,
}

pub(super) fn parse_fault(spec: &str, cpu_worker: bool) -> Option<(FaultKind, FaultPhase)> {
    let (spec, gpu_only) = match spec.strip_suffix(":gpu") {
        Some(rest) => (rest, true),
        None => (spec, false),
    };
    if gpu_only && cpu_worker {
        return None;
    }
    let (kind, phase) = spec.split_once('@')?;
    let kind = match kind {
        "abort" => FaultKind::Abort,
        "segv" => FaultKind::Segv,
        "hang" => FaultKind::Hang,
        _ => return None,
    };
    let phase = match phase {
        "init" => FaultPhase::Init,
        "load" => FaultPhase::Load,
        "run" => FaultPhase::Run,
        _ => return None,
    };
    Some((kind, phase))
}

fn trigger(kind: FaultKind) -> ! {
    match kind {
        FaultKind::Abort => std::process::abort(),
        FaultKind::Segv => {
            // Not a real memory fault: a deliberate crash that works the same
            // on every platform and leaves no core dump behind.
            std::process::abort()
        }
        FaultKind::Hang => loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        },
    }
}

fn injected_fault() -> Option<(FaultKind, FaultPhase)> {
    if !cfg!(debug_assertions) {
        return None;
    }
    parse_fault(
        &std::env::var("TINGSKRIVER_STT_FAULT").ok()?,
        super::worker_forced_cpu(),
    )
}

pub(super) fn run() -> anyhow::Result<()> {
    let fault = injected_fault();
    if let Some((kind, FaultPhase::Init)) = fault {
        trigger(kind);
    }
    let mut requests = 0u64;
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = BufWriter::new(std::io::stdout().lock());
    let mut model: Option<BoxedModel> = None;
    let mut vad: Option<crate::audio::vad::LocalVadProcessor> = None;
    let mut whisper: Option<crate::whisper_engine::WhisperEngine> = None;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    while let Some(frame) = ipc::read_frame(&mut input)? {
        requests += 1;
        match fault {
            Some((kind, FaultPhase::Load)) if requests == 1 => trigger(kind),
            Some((kind, FaultPhase::Run)) if requests > 1 => trigger(kind),
            _ => {}
        }
        let request: Request = serde_json::from_slice(&frame.control)?;
        let response = match request {
            Request::Whisper { directory, name } => {
                let result = runtime.block_on(async {
                    let engine =
                        crate::whisper_engine::WhisperEngine::new_with_models_dir(Some(directory))?;
                    engine.discover_models().await?;
                    engine.load_model(&name).await?;
                    Ok::<_, anyhow::Error>(engine)
                });
                match result {
                    Ok(engine) => {
                        whisper = Some(engine);
                        serde_json::Value::Null
                    }
                    Err(_) => serde_json::json!({"error": "Den lokale model kunne ikke indlæses."}),
                }
            }
            Request::WhisperTranscribe {
                language,
                confidence,
            } => {
                let result = if let Some(engine) = whisper.as_ref() {
                    runtime
                        .block_on(async {
                            if confidence {
                                engine
                                    .transcribe_audio_with_confidence(frame.samples, language)
                                    .await
                            } else {
                                engine
                                    .transcribe_audio(frame.samples, language)
                                    .await
                                    .map(|text| (text, 0.0, false))
                            }
                        })
                        .map_err(|_| "Lokal transskription mislykkedes.".to_string())
                } else {
                    Err("Ingen lokal model indlæst.".to_string())
                };
                serde_json::to_value(result)?
            }
            Request::Vad { rate, config } => {
                match initialize_ort().and_then(|()| {
                    crate::audio::vad::LocalVadProcessor::with_config(rate, config.into())
                }) {
                    Ok(loaded) => {
                        vad = Some(loaded);
                        serde_json::Value::Null
                    }
                    Err(_) => serde_json::json!({"error": "Taledetektion kunne ikke starte."}),
                }
            }
            Request::VadProcess { flush } => {
                let result = vad
                    .as_mut()
                    .ok_or_else(|| "Taledetektion er ikke startet.".to_string())
                    .and_then(|v| {
                        if flush {
                            v.flush()
                        } else {
                            v.process_audio(&frame.samples)
                        }
                        .map_err(|e| e.to_string())
                    });
                let mut samples = Vec::new();
                let metadata = result.map(|segments| {
                    segments
                        .into_iter()
                        .map(|s| {
                            let length = s.samples.len();
                            samples.extend(s.samples);
                            (
                                length,
                                s.start_timestamp_ms,
                                s.end_timestamp_ms,
                                s.confidence,
                            )
                        })
                        .collect::<Vec<_>>()
                });
                ipc::write_frame(
                    &mut output,
                    frame.id,
                    &serde_json::to_vec(&metadata)?,
                    &samples,
                )?;
                continue;
            }
            Request::VadBuffered => {
                serde_json::to_value(vad.as_ref().map_or(0, |v| v.buffered_session_samples()))?
            }
            Request::Parakeet { path, quantized } => {
                let loaded = initialize_ort().and_then(|()| {
                    crate::parakeet_engine::ParakeetModel::new(&path, quantized)
                        .map_err(anyhow::Error::from)
                });
                match loaded {
                    Ok(loaded) => {
                        model = Some(Box::new(loaded));
                        serde_json::Value::Null
                    }
                    Err(_) => serde_json::json!({"error": "Den lokale model kunne ikke indlæses."}),
                }
            }
            Request::WhisperTimed { language } => {
                let result = if let Some(engine) = whisper.as_ref() {
                    runtime
                        .block_on(engine.transcribe_audio_timed(frame.samples, language))
                        .map_err(|_| "Lokal transskription med ordtider mislykkedes.".to_string())
                } else {
                    Err("Ingen lokal model indlæst.".to_string())
                };
                serde_json::to_value(result)?
            }
            Request::TranscribeTimed => {
                let result = model
                    .as_mut()
                    .ok_or_else(|| "Ingen lokal model indlæst.".to_string())
                    .and_then(|m| m.transcribe_timed(frame.samples));
                serde_json::to_value(result)?
            }
            Request::Transcribe => {
                let result = model
                    .as_mut()
                    .ok_or_else(|| "Ingen lokal model indlæst.".to_string())
                    .and_then(|m| m.transcribe(frame.samples));
                serde_json::to_value(result)?
            }
            Request::Echo => {
                ipc::write_frame(&mut output, frame.id, b"null", &frame.samples)?;
                continue;
            }
        };
        ipc::write_frame(&mut output, frame.id, &serde_json::to_vec(&response)?, &[])?;
    }
    Ok(())
}

fn initialize_ort() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::var_os("TINGSKRIVER_STT_FORCE_ORT_FAILURE").is_none(),
        "ONNX Runtime startup forced to fail"
    );
    #[cfg(windows)]
    {
        let path = std::env::var_os("ORT_DYLIB_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or(std::env::current_exe()?.with_file_name("onnxruntime.dll"));
        ort::init_from(path.to_string_lossy().into_owned())
            .with_telemetry(false)
            .commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fault_specs() {
        assert_eq!(
            parse_fault("hang@run", false),
            Some((FaultKind::Hang, FaultPhase::Run))
        );
        assert_eq!(
            parse_fault("abort@load:gpu", false),
            Some((FaultKind::Abort, FaultPhase::Load))
        );
        assert_eq!(parse_fault("abort@load:gpu", true), None);
        assert_eq!(
            parse_fault("segv@init", true).map(|f| f.0),
            Some(FaultKind::Segv)
        );
        assert_eq!(parse_fault("nonsense", false), None);
        assert_eq!(parse_fault("abort@feed", false), None);
    }
}
