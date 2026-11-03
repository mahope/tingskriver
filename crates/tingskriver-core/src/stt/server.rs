use super::{ipc, Request};
use crate::parakeet_engine::pool::BoxedModel;
use std::io::{BufReader, BufWriter};

pub(super) fn run() -> anyhow::Result<()> {
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = BufWriter::new(std::io::stdout().lock());
    let mut model: Option<BoxedModel> = None;
    let mut vad: Option<crate::audio::vad::LocalVadProcessor> = None;
    let mut whisper: Option<crate::whisper_engine::WhisperEngine> = None;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    while let Some(frame) = ipc::read_frame(&mut input)? {
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
