//! Repeat the newest saved dictation without changing its history entry.

use std::future::Future;

use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::store::{Store, AUDIO_RETENTION_MS};

pub const COMPARISON_EVENT: &str = "dictation-rerun-comparison";
pub const AUDIO_UNAVAILABLE_MESSAGE: &str =
    "Lyden er slettet eller ældre end 24 timer. Der blev ikke indsat noget.";

pub const CLOUD_REFUSED_MESSAGE: &str =
    "Lyden må ikke sendes til Syv.ai: den oprindelige tilstand var kun lokal eller er ukendt.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub history_id: i64,
    pub before: String,
    pub after: String,
    pub changed: bool,
}

async fn latest(store: &Store) -> Result<super::store::HistoryEntry, String> {
    store
        .history(1)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "Ingen diktering i historikken endnu".into())
}

pub async fn last_text(store: &Store) -> Result<String, String> {
    nonempty(latest(store).await?.text)
}

fn nonempty(text: String) -> Result<String, String> {
    if text.trim().is_empty() {
        Err("Teksten er tom. Der blev ikke indsat noget.".into())
    } else {
        Ok(text)
    }
}

/// Rerun exactly the newest entry, never silently choose an older recording.
/// The caller chooses another provider or mode from the previous engine.
/// No text is inserted or persisted; the comparison must be reviewed first.
pub async fn rerun<F, Fut>(
    store: &Store,
    now: i64,
    preference: &str,
    provider: F,
) -> Result<Comparison, String>
where
    F: FnOnce(super::store::HistoryEntry, Vec<u8>) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    let entry = latest(store).await?;
    if now.saturating_sub(entry.created_at) >= AUDIO_RETENTION_MS {
        return Err(AUDIO_UNAVAILABLE_MESSAGE.into());
    }
    let path: Option<String> =
        sqlx::query("SELECT audio_path FROM dictation_history WHERE id = ?1")
            .bind(entry.id)
            .fetch_optional(&store.pool())
            .await
            .map_err(|e| format!("Databasefejl (diktering): {e}"))?
            .and_then(|row| row.get("audio_path"));
    let wav = tokio::fs::read(path.ok_or(AUDIO_UNAVAILABLE_MESSAGE)?)
        .await
        .map_err(|_| AUDIO_UNAVAILABLE_MESSAGE.to_string())?;
    if wav.is_empty() {
        return Err(AUDIO_UNAVAILABLE_MESSAGE.into());
    }
    alternate_engine(&entry.engine, preference, entry.provider.as_deref())?;
    let after = nonempty(provider(entry.clone(), wav).await?)?;
    Ok(Comparison {
        history_id: entry.id,
        changed: entry.text != after,
        before: entry.text,
        after,
    })
}

/// Execute a registered shortcut. The host surfaces errors through its overlay.
pub async fn shortcut(
    action: crate::shortcuts::ShortcutAction,
    service: &super::service::Service,
    host: &crate::host::HostHandle,
) -> Result<(), String> {
    use crate::host::HostExt;
    use crate::shortcuts::ShortcutAction;
    match action {
        ShortcutAction::InsertLast => {
            // Capture focus before awaiting the history database.
            #[cfg(windows)]
            let target = super::win::foreground_window();
            let text = last_text(&service.store).await?;
            #[cfg(windows)]
            {
                let settings = service.settings();
                tokio::task::spawn_blocking(move || {
                    let options = super::win::InsertOptions {
                        restore_after: std::time::Duration::from_millis(
                            settings.restore_clipboard_ms,
                        ),
                        paste_timeout: std::time::Duration::from_millis(settings.paste_timeout_ms),
                        app: super::win::foreground_process(),
                    };
                    super::win::insert_output_at(&text, &options, false, target, None).map(|_| ())
                })
                .await
                .map_err(|e| e.to_string())?
            }
            #[cfg(not(windows))]
            {
                let _ = text;
                Err("Indsættelse understøttes kun på Windows".into())
            }
        }
        ShortcutAction::RerunLast => {
            let settings = service.settings();
            let preference = settings.engine.clone();
            let diff = rerun(
                &service.store,
                chrono::Utc::now().timestamp_millis(),
                &preference,
                |previous, wav| async move {
                    match alternate_engine(
                        &previous.engine,
                        &settings.engine,
                        previous.provider.as_deref(),
                    )? {
                        super::logic::engine::Engine::Syv => {
                            let key = super::secrets::syv_key()
                                .ok_or("En anden udbyder kræver en Syv.ai-nøgle")?;
                            super::syv::transcribe_language(
                                &service.http,
                                &settings.syv_endpoint,
                                &key,
                                previous.syv_model.as_deref().unwrap_or(&settings.syv_model),
                                wav,
                                None,
                                previous.language.as_deref().unwrap_or(&settings.language),
                            )
                            .await
                        }
                        super::logic::engine::Engine::Local => {
                            transcribe_local_with(
                                &wav,
                                || async { super::warm::ensure_parakeet().await.map(|_| ()) },
                                crate::parakeet_engine::commands::parakeet_transcribe_audio,
                            )
                            .await
                        }
                    }
                },
            )
            .await?;
            host.emit(COMPARISON_EVENT, diff)
        }
        _ => Err("Genvejen er ikke en gentagelse".into()),
    }
}

async fn transcribe_local_with<Ready, ReadyFuture, Transcribe, TranscriptFuture>(
    wav: &[u8],
    ready: Ready,
    transcribe: Transcribe,
) -> Result<String, String>
where
    Ready: FnOnce() -> ReadyFuture,
    ReadyFuture: Future<Output = Result<(), String>>,
    Transcribe: FnOnce(Vec<f32>) -> TranscriptFuture,
    TranscriptFuture: Future<Output = Result<String, String>>,
{
    let samples = decode_saved_wav(wav)?;
    ready().await?;
    transcribe(samples).await
}

fn alternate_engine(
    previous: &str,
    preference: &str,
    original: Option<&str>,
) -> Result<super::logic::engine::Engine, String> {
    use super::logic::engine::{Engine, EnginePreference};
    match previous {
        "local" if EnginePreference::parse(preference) == EnginePreference::LocalOnly => {
            Err("Vælg en anden udbyder. Kun lokal behandling er slået til.".into())
        }
        "local" if !matches!(original, Some("auto" | "syv")) => Err(CLOUD_REFUSED_MESSAGE.into()),
        "local" => Ok(Engine::Syv),
        "syv" => Ok(Engine::Local),
        _ => Err("Den tidligere udbyder understøtter ikke genkørsel endnu".into()),
    }
}

// History audio is written by logic::audio::wav_bytes: mono PCM16 at 16 kHz.
fn decode_saved_wav(wav: &[u8]) -> Result<Vec<f32>, String> {
    if wav.len() < 44
        || &wav[..4] != b"RIFF"
        || &wav[8..16] != b"WAVEfmt "
        || wav[16..20] != 16u32.to_le_bytes()
        || wav[20..24] != [1, 0, 1, 0]
        || wav[24..28] != 16_000u32.to_le_bytes()
        || wav[32..36] != [2, 0, 16, 0]
        || &wav[36..40] != b"data"
        || (wav.len() - 44) % 2 != 0
        || u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize != wav.len() - 44
    {
        return Err("Den gemte lyd kunne ikke læses".into());
    }
    if wav.len() == 44 {
        return Err("Den gemte lyd er tom".into());
    }
    Ok(wav[44..]
        .chunks_exact(2)
        .map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / i16::MAX as f32)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::store::NewHistoryEntry;

    #[tokio::test]
    async fn cold_local_model_is_ready_before_transcription() {
        let ready = std::sync::atomic::AtomicBool::new(false);
        let wav = crate::dictation::logic::audio::wav_bytes(&[0.5], 16_000);
        let text = transcribe_local_with(
            &wav,
            || async {
                ready.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
            |samples| {
                assert!(ready.load(std::sync::atomic::Ordering::SeqCst));
                assert_eq!(samples.len(), 1);
                async { Ok("Kold model klar".into()) }
            },
        )
        .await
        .unwrap();
        assert_eq!(text, "Kold model klar");
    }

    #[tokio::test]
    async fn failed_model_load_never_transcribes() {
        let wav = crate::dictation::logic::audio::wav_bytes(&[0.5], 16_000);
        let error = transcribe_local_with(
            &wav,
            || async { Err("Modellen kunne ikke indlæses".into()) },
            |_| async { panic!("must not transcribe without a ready model") },
        )
        .await
        .unwrap_err();
        assert_eq!(error, "Modellen kunne ikke indlæses");
    }

    async fn fixture(text: &str, path: Option<&str>, at: i64) -> Store {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let store = Store::with_pool(pool).await.unwrap();
        store
            .add_history(NewHistoryEntry {
                created_at: at,
                text: text.into(),
                app: None,
                style: "raw".into(),
                engine: "local".into(),
                provider: Some("auto".into()),
                syv_model: None,
                language: None,
                fallback_reason: None,
                latency_ms: 0,
                audio_ms: 100,
                insert_method: None,
                audio_path: path.map(str::to_owned),
            })
            .await
            .unwrap();
        store
    }

    #[test]
    fn switches_provider_without_overriding_local_only() {
        use crate::dictation::logic::engine::Engine;
        assert_eq!(
            alternate_engine("local", "auto", Some("auto")).unwrap(),
            Engine::Syv
        );
        assert_eq!(
            alternate_engine("syv", "auto", Some("auto")).unwrap(),
            Engine::Local
        );
        assert_eq!(
            alternate_engine("syv", "local", Some("auto")).unwrap(),
            Engine::Local
        );
        assert!(alternate_engine("local", "local", Some("auto")).is_err());
        assert!(alternate_engine("local", "local-only", Some("auto")).is_err());
        assert!(alternate_engine("unknown", "auto", Some("auto")).is_err());
    }

    #[tokio::test]
    async fn local_or_unknown_origin_never_calls_cloud_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last.wav");
        tokio::fs::write(&path, b"fake wav").await.unwrap();
        let store = fixture("Tekst", path.to_str(), 0).await;
        // An available Syv key must not bypass the original privacy preference.
        let syv_key = Some("test-key");
        for original in [
            Some("local"),
            Some("local-only"),
            None,
            Some("unrecognized"),
        ] {
            sqlx::query("UPDATE dictation_history SET provider = ?1")
                .bind(original)
                .execute(&store.pool())
                .await
                .unwrap();
            assert!(syv_key.is_some());
            let error = rerun(&store, 1, "auto", |_, _| async {
                panic!("cloud provider must never be called");
                #[allow(unreachable_code)]
                Ok(String::new())
            })
            .await
            .unwrap_err();
            assert!(error.contains("oprindelige tilstand"), "{error}");
        }
    }

    #[tokio::test]
    async fn rerun_preserves_resolved_model_and_language() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last.wav");
        tokio::fs::write(&path, b"fake wav").await.unwrap();
        let store = fixture("Tekst", path.to_str(), 0).await;
        sqlx::query("UPDATE dictation_history SET syv_model = 'original-model', language = 'da'")
            .execute(&store.pool())
            .await
            .unwrap();
        rerun(&store, 1, "auto", |entry, _| async move {
            assert_eq!(entry.syv_model.as_deref(), Some("original-model"));
            assert_eq!(entry.language.as_deref(), Some("da"));
            Ok("Ny tekst".into())
        })
        .await
        .unwrap();
    }

    #[test]
    fn decodes_only_saved_pcm_format() {
        let wav = crate::dictation::logic::audio::wav_bytes(&[0.5, -0.5], 16_000);
        let samples = decode_saved_wav(&wav).unwrap();
        assert!((samples[0] - 0.5).abs() < 0.001);
        assert!((samples[1] + 0.5).abs() < 0.001);
        assert!(decode_saved_wav(&wav[..43]).is_err());
        assert!(decode_saved_wav(&crate::dictation::logic::audio::wav_bytes(&[], 16_000)).is_err());
        assert!(
            decode_saved_wav(&crate::dictation::logic::audio::wav_bytes(&[0.5], 48_000)).is_err()
        );
    }

    #[tokio::test]
    async fn expiration_boundary_and_unchanged_comparison() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last.wav");
        tokio::fs::write(&path, b"wav").await.unwrap();
        let store = fixture("Tekst", path.to_str(), 0).await;
        let diff = rerun(&store, AUDIO_RETENTION_MS - 1, "auto", |_, _| async {
            Ok("Tekst".into())
        })
        .await
        .unwrap();
        assert!(!diff.changed);
        assert!(rerun(&store, AUDIO_RETENTION_MS, "auto", |_, _| async {
            panic!("expired audio");
            #[allow(unreachable_code)]
            Ok(String::new())
        })
        .await
        .is_err());
    }

    #[tokio::test]
    async fn latest_entry_wins_even_without_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("older.wav");
        tokio::fs::write(&path, b"wav").await.unwrap();
        let store = fixture("Ældre", path.to_str(), 10).await;
        store
            .add_history(NewHistoryEntry {
                created_at: 10,
                text: "Nyeste".into(),
                app: None,
                style: "raw".into(),
                engine: "local".into(),
                provider: Some("auto".into()),
                syv_model: None,
                language: None,
                fallback_reason: None,
                latency_ms: 0,
                audio_ms: 100,
                insert_method: None,
                audio_path: None,
            })
            .await
            .unwrap();
        assert_eq!(last_text(&store).await.unwrap(), "Nyeste");
        assert!(rerun(&store, 11, "auto", |_, _| async {
            panic!("must not use older audio");
            #[allow(unreachable_code)]
            Ok(String::new())
        })
        .await
        .is_err());
    }

    #[tokio::test]
    async fn repeats_saved_text_and_rejects_empty_history() {
        let store = fixture("Sidste tekst", None, 0).await;
        assert_eq!(last_text(&store).await.unwrap(), "Sidste tekst");
        store.clear_history().await.unwrap();
        assert!(last_text(&store).await.unwrap_err().contains("Ingen"));
        let store = fixture("  ", None, 0).await;
        assert!(last_text(&store).await.is_err());
    }

    #[tokio::test]
    async fn fake_provider_receives_audio_and_returns_comparison() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last.wav");
        tokio::fs::write(&path, b"fake wav").await.unwrap();
        let store = fixture("Før", path.to_str(), 100).await;
        let diff = rerun(&store, 101, "auto", |engine, wav| async move {
            assert_eq!(engine.engine, "local");
            assert_eq!(wav, b"fake wav");
            Ok("Efter".into())
        })
        .await
        .unwrap();
        assert_eq!(diff.before, "Før");
        assert_eq!(diff.after, "Efter");
        assert!(diff.changed);
        assert_eq!(last_text(&store).await.unwrap(), "Før");
    }

    #[tokio::test]
    async fn unavailable_audio_never_calls_provider() {
        for (path, now) in [
            (None, 1),
            (Some("missing.wav"), 1),
            (Some("missing.wav"), AUDIO_RETENTION_MS + 1),
        ] {
            let store = fixture("Tekst", path, 0).await;
            let error = rerun(&store, now, "auto", |_, _| async {
                panic!("provider must not run");
                #[allow(unreachable_code)]
                Ok(String::new())
            })
            .await
            .unwrap_err();
            assert!(error.contains("Lyden"), "{error}");
        }
    }

    #[tokio::test]
    async fn provider_failure_and_empty_output_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last.wav");
        tokio::fs::write(&path, b"wav").await.unwrap();
        let store = fixture("Tekst", path.to_str(), 0).await;
        assert!(rerun(&store, 1, "auto", |_, _| async {
            Err("Udbyderen svarede ikke".into())
        })
        .await
        .is_err());
        assert!(rerun(&store, 1, "auto", |_, _| async { Ok("  ".into()) })
            .await
            .is_err());
    }
}
