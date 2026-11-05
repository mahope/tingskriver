//! Headless measurement of local transcription models (`tingskriver-bench`).
//! Uses the same `Sample` import, engine calls, Danish language setting and WER
//! scoring as the Måling page, but needs no UI. See `docs/measurement.md`.
use super::Sample;
use crate::wer::{self, runner::ModelEngine, Score};
use serde::Serialize;
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::Instant,
};

pub const MAX_REPEAT: usize = 100;

#[derive(Debug, PartialEq)]
pub struct Options {
    pub file: Option<PathBuf>,
    pub models: Vec<String>,
    pub repeat: usize,
    pub json: bool,
    pub list_models: bool,
    pub models_dir: Option<PathBuf>,
    pub reference: Option<PathBuf>,
}

pub const USAGE: &str =
    "Brug: tingskriver-bench --file <WAV> --model <navn> [--model <navn> ...]\n\
    \x20      [--repeat N] [--reference <txt>] [--models-dir <mappe>] [--json]\n\
    \x20      tingskriver-bench --list-models [--models-dir <mappe>] [--json]\n\
    WAV: mono, 16 kHz, PCM16, højst 90 sekunder. Modeller indlæses kun, hvis de er installeret.";

pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        file: None,
        models: vec![],
        repeat: 1,
        json: false,
        list_models: false,
        models_dir: None,
        reference: None,
    };
    let mut it = args.iter();
    while let Some(key) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} mangler en værdi"))
        };
        match key.as_str() {
            "--json" => o.json = true,
            "--list-models" => o.list_models = true,
            "--file" => o.file = Some(value("--file")?.into()),
            "--model" => o.models.push(value("--model")?),
            "--models-dir" => o.models_dir = Some(value("--models-dir")?.into()),
            "--reference" => o.reference = Some(value("--reference")?.into()),
            "--repeat" => {
                o.repeat = value("--repeat")?
                    .parse()
                    .ok()
                    .filter(|n| (1..=MAX_REPEAT).contains(n))
                    .ok_or_else(|| format!("--repeat skal være 1–{MAX_REPEAT}"))?
            }
            other => return Err(format!("Ukendt argument: {other}")),
        }
    }
    if !o.list_models && (o.file.is_none() || o.models.is_empty()) {
        return Err("--file og mindst én --model kræves (eller --list-models)".into());
    }
    Ok(o)
}

/// Latency summary over repeated runs of the same audio.
#[derive(Debug, Serialize, PartialEq)]
pub struct Timing {
    pub runs_ms: Vec<f64>,
    pub best_ms: f64,
    pub median_ms: f64,
    pub mean_ms: f64,
    /// Real-time factor: audio seconds / processing seconds (higher is faster).
    pub rtf_best: f64,
    pub rtf_median: f64,
}

pub fn summarize(runs_ms: &[f64], audio_seconds: f64) -> Option<Timing> {
    if runs_ms.is_empty() || runs_ms.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return None;
    }
    let mut sorted = runs_ms.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    let rtf = |ms: f64| audio_seconds / (ms / 1000.0);
    Some(Timing {
        runs_ms: runs_ms.to_vec(),
        best_ms: sorted[0],
        median_ms: median,
        mean_ms: runs_ms.iter().sum::<f64>() / n as f64,
        rtf_best: rtf(sorted[0]),
        rtf_median: rtf(median),
    })
}

#[derive(Debug, Serialize)]
pub struct ModelReport {
    pub model: String,
    pub engine: String,
    /// "completed", "skipped" or "failed".
    pub status: &'static str,
    pub message: String,
    pub load_ms: Option<f64>,
    pub timing: Option<Timing>,
    pub wer: Option<f64>,
    pub score: Option<Score>,
    /// Process peak resident memory after this model ran (Linux only). It never
    /// decreases, so run one model per process for a clean per-model figure.
    pub peak_rss_mb: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub created_at: String,
    pub normalization: &'static str,
    pub platform: String,
    pub audio_seconds: f64,
    pub repeat: usize,
    pub results: Vec<ModelReport>,
}

pub fn engine_for(model: &str) -> &'static str {
    // Same routing as the Måling page.
    if model.starts_with("parakeet-") {
        "parakeet"
    } else {
        "whisper"
    }
}

/// Peak resident memory of this process in MB; `None` where unsupported.
#[allow(clippy::disallowed_methods)] // tiny /proc read at exit, not on an async hot path
pub fn peak_rss_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: u64 = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()?;
    Some(kb / 1024)
}

/// Time `repeat` runs of `transcribe`; returns timings and the last hypothesis.
/// Any failed run fails the whole measurement (no partial statistics).
pub async fn time_runs<F, Fut>(repeat: usize, mut transcribe: F) -> Result<(Vec<f64>, String), ()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<String, ()>>,
{
    let mut runs = Vec::with_capacity(repeat);
    let mut text = String::new();
    for _ in 0..repeat {
        let start = Instant::now();
        text = transcribe().await?;
        runs.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    Ok((runs, text))
}

pub fn finish_report(
    model: &str,
    engine: &str,
    load_ms: Option<f64>,
    runs: Result<(Vec<f64>, String), ()>,
    audio_seconds: f64,
    reference: Option<&str>,
) -> ModelReport {
    let mut r = ModelReport {
        model: model.into(),
        engine: engine.into(),
        status: "failed",
        message: "Målingen fejlede.".into(),
        load_ms,
        timing: None,
        wer: None,
        score: None,
        peak_rss_mb: peak_rss_mb(),
    };
    if let Ok((runs_ms, text)) = runs {
        if let Some(timing) = summarize(&runs_ms, audio_seconds) {
            r.timing = Some(timing);
            r.status = "completed";
            r.message = "Målt".into();
            if let Some(reference) = reference {
                let score = wer::score(reference, &text);
                r.wer = score.wer();
                r.score = Some(score);
            }
        }
    }
    r
}

pub async fn run_model(
    model: &str,
    models_dir: &Path,
    sample: &Sample,
    repeat: usize,
    reference: Option<&str>,
) -> ModelReport {
    let engine_name = engine_for(model);
    let start = Instant::now();
    let loaded = ModelEngine::load(engine_name, models_dir.to_path_buf(), model.into()).await;
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    let engine = match loaded {
        Ok(e) => e,
        Err(_) => {
            return ModelReport {
                model: model.into(),
                engine: engine_name.into(),
                status: "skipped",
                message: "Modellen kunne ikke indlæses. Installér den først (intet downloades)."
                    .into(),
                load_ms: None,
                timing: None,
                wer: None,
                score: None,
                peak_rss_mb: None,
            }
        }
    };
    let runs = time_runs(repeat, || async {
        match &engine {
            ModelEngine::Parakeet(e, _) => e.transcribe_audio(sample.samples.clone()).await,
            ModelEngine::Whisper(e, _) => e
                .transcribe_audio_with_confidence(sample.samples.clone(), Some("da".into()))
                .await
                .map(|(t, _, _)| t),
        }
        .map_err(|_| ())
    })
    .await;
    finish_report(
        model,
        engine_name,
        Some(load_ms),
        runs,
        sample.duration_seconds(),
        reference,
    )
}

pub fn new_report(audio_seconds: f64, repeat: usize, results: Vec<ModelReport>) -> Report {
    Report {
        schema_version: 1,
        created_at: chrono::Utc::now().to_rfc3339(),
        normalization: "unicode-alphanumeric-lower-v1",
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        audio_seconds,
        repeat,
        results,
    }
}

/// Installed (or known) model names per engine; no downloads.
pub async fn list_models(models_dir: &Path) -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    if let Ok(e) =
        crate::parakeet_engine::ParakeetEngine::new_with_models_dir(Some(models_dir.into()))
    {
        for m in e.discover_models().await.unwrap_or_default() {
            out.push((m.name, "parakeet", format!("{:?}", m.status)));
        }
    }
    if let Ok(e) =
        crate::whisper_engine::WhisperEngine::new_with_models_dir(Some(models_dir.into()))
    {
        for m in e.discover_models().await.unwrap_or_default() {
            out.push((m.name, "whisper", format!("{:?}", m.status)));
        }
    }
    out
}

pub fn render_text(report: &Report) -> String {
    let mut s = format!(
        "Lyd: {:.1} s, {} kørsel(er), {}\n",
        report.audio_seconds, report.repeat, report.platform
    );
    for r in &report.results {
        s += &format!("{} [{}]: {}", r.model, r.engine, r.status);
        if let Some(t) = &r.timing {
            s += &format!(
                " bedste {:.0} ms (RTF {:.2}), median {:.0} ms (RTF {:.2})",
                t.best_ms, t.rtf_best, t.median_ms, t.rtf_median
            );
        }
        if let Some(w) = r.wer {
            s += &format!(", WER {:.2} %", w * 100.0);
        }
        if let Some(m) = r.peak_rss_mb {
            s += &format!(", spids-RSS {m} MB");
        }
        if r.status != "completed" {
            s += &format!(" ({})", r.message);
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn median_and_best_of_five_runs() {
        let t = summarize(&[300.0, 100.0, 500.0, 200.0, 400.0], 30.0).unwrap();
        assert_eq!(t.best_ms, 100.0);
        assert_eq!(t.median_ms, 300.0);
        assert_eq!(t.mean_ms, 300.0);
        assert!((t.rtf_best - 300.0).abs() < 1e-9);
        assert!((t.rtf_median - 100.0).abs() < 1e-9);
        assert_eq!(t.runs_ms.len(), 5);
    }

    #[test]
    fn even_count_median_is_midpoint_and_bad_input_rejected() {
        assert_eq!(summarize(&[100.0, 300.0], 1.0).unwrap().median_ms, 200.0);
        assert!(summarize(&[], 1.0).is_none());
        assert!(summarize(&[f64::NAN], 1.0).is_none());
        assert!(summarize(&[0.0], 1.0).is_none());
    }

    #[test]
    fn parses_arguments_and_rejects_bad_ones() {
        let o = parse_args(&a(&[
            "--file", "x.wav", "--model", "m", "--repeat", "5", "--json",
        ]))
        .unwrap();
        assert_eq!((o.repeat, o.json, o.models.len()), (5, true, 1));
        assert!(parse_args(&a(&["--list-models"])).unwrap().list_models);
        assert!(parse_args(&a(&["--file", "x.wav"])).is_err());
        assert!(parse_args(&a(&["--file", "x", "--model", "m", "--repeat", "0"])).is_err());
        assert!(parse_args(&a(&["--file", "x", "--model", "m", "--repeat", "101"])).is_err());
        assert!(parse_args(&a(&["--bogus"])).is_err());
        assert!(parse_args(&a(&["--file"])).is_err());
    }

    #[test]
    fn engine_routing_matches_measurement_page() {
        assert_eq!(engine_for("parakeet-tdt-0.6b-v3-int8"), "parakeet");
        assert_eq!(engine_for("large-v3-turbo"), "whisper");
    }

    #[tokio::test]
    async fn report_has_wer_only_with_reference_and_fails_on_error() {
        let runs = time_runs(3, || async {
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            Ok("rød grød".to_string())
        })
        .await;
        let r = finish_report(
            "m",
            "whisper",
            Some(1.0),
            runs,
            2.0,
            Some("rød grød med fløde"),
        );
        assert_eq!(r.status, "completed");
        assert_eq!(r.timing.as_ref().unwrap().runs_ms.len(), 3);
        assert!((r.wer.unwrap() - 0.5).abs() < 1e-9);

        let runs = time_runs(2, || async { Ok("x".to_string()) }).await;
        assert!(finish_report("m", "whisper", None, runs, 1.0, None)
            .wer
            .is_none());

        let mut n = 0;
        let runs = time_runs(3, || {
            n += 1;
            let fail = n == 2;
            async move {
                if fail {
                    Err(())
                } else {
                    Ok("x".to_string())
                }
            }
        })
        .await;
        let r = finish_report("m", "whisper", None, runs, 1.0, None);
        assert_eq!(r.status, "failed");
        assert!(r.timing.is_none());
    }
}
