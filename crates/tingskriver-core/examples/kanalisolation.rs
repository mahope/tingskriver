//! Channel isolation between two WAV tracks with pilot tones (#189).
//!
//! ```text
//! kanalisolation <microphone.wav> <system.wav> <thresholds.json> [phase]...
//! kanalisolation microphone.wav system.wav docs/kanalisolation-graenser.json \
//!     both:0:10 mic:10:20 system:20:30
//! ```
//!
//! With no phases, Goertzel onset detection finds both, microphone, then system.
//! A phase is `both`, `mic` or `system` followed by start and end seconds on the
//! recording clock. Exit code 0 means pass, 1 means leakage/clipping/duration
//! failure and 2 means unusable input. `--json` prints the report as JSON.
use std::{path::Path, process::ExitCode};
use tingskriver_core::audio::isolation::{analyze, detect_phases, read_wav, Phase, Thresholds};

fn load(path: &str) -> Result<(Vec<f32>, u32), String> {
    let bytes = std::fs::read(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    read_wav(&bytes).map_err(|e| format!("{path}: {e}"))
}

fn run(args: &[String]) -> Result<bool, String> {
    let json = args.iter().any(|a| a == "--json");
    let args: Vec<_> = args.iter().filter(|a| *a != "--json").collect();
    let [mic, system, thresholds, phases @ ..] = args.as_slice() else {
        return Err(
            "usage: kanalisolation <microphone.wav> <system.wav> <thresholds.json> [phase]..."
                .into(),
        );
    };
    let thresholds = Thresholds::from_json(
        &std::fs::read_to_string(thresholds).map_err(|e| format!("{thresholds}: {e}"))?,
    )?;
    let phases = phases
        .iter()
        .map(|p| Phase::parse(p))
        .collect::<Result<Vec<_>, _>>()?;
    let ((mic, mic_rate), (system, system_rate)) = (load(mic)?, load(system)?);
    if mic_rate != system_rate {
        return Err(format!("sample rates differ: {mic_rate} and {system_rate}"));
    }
    let phases = if phases.is_empty() {
        detect_phases(&mic, &system, mic_rate, &thresholds)?
    } else {
        phases
    };
    let report = analyze(&mic, &system, mic_rate, &phases, &thresholds);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        print!("{}", report.to_text());
    }
    Ok(report.passed())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
