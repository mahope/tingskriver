//! Run synthetic, local-only report evaluation; never print source or report text.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tingskriver_core::summary::{
    map_reduce::{
        self,
        eval::{self, Fixture, Meter},
        Budget, Builtin, Model,
    },
    summary_engine::client,
};

fn options() -> Result<BTreeMap<String, String>, String> {
    let mut args = std::env::args().skip(1);
    let mut options = BTreeMap::new();
    while let Some(key) = args.next() {
        if ![
            "--generer",
            "--transskripter",
            "--motor",
            "--out",
            "--data-dir",
            "--model",
            "--template",
            "--backend",
            "--worker",
            "--result",
            "--referater",
        ]
        .contains(&key.as_str())
        {
            return Err("Ukendt argument".into());
        }
        options.insert(key, args.next().ok_or("Argument mangler værdi")?);
    }
    Ok(options)
}
fn required<'a>(options: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    options
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("Mangler {key}"))
}
fn value<'a>(options: &'a BTreeMap<String, String>, key: &str, default: &'a str) -> &'a str {
    options.get(key).map(String::as_str).unwrap_or(default)
}
#[derive(Serialize, Deserialize)]
struct Record {
    id: String,
    minutes: usize,
    engine: String,
    model: String,
    requested_backend: String,
    vulkan_active: Option<bool>,
    total_seconds: f64,
    process_peak_rss_bytes_sampled: Option<u64>,
    helper_peak_rss_bytes_sampled: Option<u64>,
    generated_tokens: Option<u64>,
    generation_seconds: Option<f64>,
    tokens_per_second: Option<f64>,
    estimated_output_tokens: usize,
    estimated_tokens_per_second_end_to_end: f64,
    stats: map_reduce::Stats,
    calls: Vec<eval::Call>,
    quality: eval::Quality,
}
async fn worker(options: &BTreeMap<String, String>) -> Result<(), String> {
    let fixture: Fixture = serde_json::from_slice(
        &fs::read(required(options, "--worker")?).map_err(|_| "Kan ikke læse testfil")?,
    )
    .map_err(|_| "Ugyldigt testformat")?;
    let engine = required(options, "--motor")?;
    let model_name = value(options, "--model", "qwen3.5:4b");
    let data_dir = PathBuf::from(value(options, "--data-dir", "."));
    let model: Box<dyn Model> = match engine {
        "fake" => Box::new(eval::Fake),
        "lokal" => Box::new(Builtin {
            data_dir: &data_dir,
            name: model_name,
            cancellation: None,
            reasoning_stripped: false,
        }),
        _ => return Err("Motor skal være fake eller lokal".into()),
    };
    let mut meter = Meter {
        inner: model,
        calls: Vec::new(),
    };
    let start = Instant::now();
    let result = eval::danish_report(
        &mut meter,
        &Budget::for_model(model_name)?,
        &fixture,
        value(options, "--template", "daily_standup"),
    )
    .await;
    let seconds = start.elapsed().as_secs_f64();
    if engine == "lokal" {
        client::force_shutdown_sidecar()
            .await
            .map_err(|_| "Kunne ikke stoppe helper")?;
    }
    let report = result?;
    let tokens = meter.calls.iter().map(|c| c.estimated_output_tokens).sum();
    let record = Record {
        id: fixture.id.clone(),
        minutes: fixture.minutes,
        engine: engine.into(),
        model: if engine == "fake" {
            "fake".into()
        } else {
            model_name.into()
        },
        requested_backend: value(options, "--backend", "ukendt").into(),
        vulkan_active: None,
        total_seconds: seconds,
        process_peak_rss_bytes_sampled: None,
        helper_peak_rss_bytes_sampled: None,
        generated_tokens: None,
        generation_seconds: None,
        tokens_per_second: None,
        estimated_output_tokens: tokens,
        estimated_tokens_per_second_end_to_end: tokens as f64 / seconds.max(f64::EPSILON),
        quality: eval::quality(&fixture, &report.content),
        stats: report.stats,
        calls: meter.calls,
    };
    if let Some(directory) = options.get("--referater") {
        fs::create_dir_all(directory).map_err(|_| "Kan ikke oprette referatmappe")?;
        // Use the fixture filename, never an untrusted fixture id as a path.
        let name = Path::new(required(options, "--worker")?)
            .file_stem()
            .ok_or("Ugyldigt filnavn")?;
        fs::write(
            Path::new(directory).join(name).with_extension("md"),
            report.content,
        )
        .map_err(|_| "Kan ikke gemme referat")?;
    }
    fs::write(
        required(options, "--result")?,
        serde_json::to_vec_pretty(&record).map_err(|_| "Kan ikke serialisere måling")?,
    )
    .map_err(|_| "Kan ikke gemme måling".to_string())
}
// A boxed model lets fake and local exercise exactly the same harness.
// The blanket implementation is in the library, so the example only orchestrates processes.
fn helper_stats(stderr: &str, record: &mut Record) {
    let mut tokens = Vec::new();
    let mut seconds = Vec::new();
    for line in stderr.lines() {
        if let Some((_, n)) = line.split_once("• Output tokens: ") {
            if let Ok(n) = n.trim().parse::<u64>() {
                tokens.push(n);
            }
        }
        if let Some((_, n)) = line.split_once("• Generation time: ") {
            if let Ok(n) = n.trim().trim_end_matches('s').parse::<f64>() {
                seconds.push(n);
            }
        }
    }
    if tokens.len() == record.calls.len() && seconds.len() == tokens.len() && !tokens.is_empty() {
        let total: f64 = seconds.iter().sum();
        record.generated_tokens = Some(tokens.iter().sum());
        record.generation_seconds = Some(total);
        if total > 0.0 {
            record.tokens_per_second = record.generated_tokens.map(|n| n as f64 / total);
        }
    }
    // Merely finding a Vulkan device does not prove tensor offload.
    let has_vulkan = stderr
        .lines()
        .any(|line| line.contains("Vulkan") || line.contains("VULKAN"));
    let offloaded = stderr.lines().any(|line| {
        line.contains("offloaded")
            && line.split_whitespace().any(|word| {
                word.split_once('/')
                    .is_some_and(|(n, _)| n.parse::<u32>().is_ok_and(|n| n > 0))
            })
    });
    if has_vulkan && offloaded {
        record.vulkan_active = Some(true);
    }
}
fn coordinator(options: &BTreeMap<String, String>) -> Result<(), String> {
    if !["cpu", "vulkan", "ukendt"].contains(&value(options, "--backend", "ukendt")) {
        return Err("Backend skal være cpu, vulkan eller ukendt".into());
    }
    if !["fake", "lokal"].contains(&required(options, "--motor")?) {
        return Err("Motor skal være fake eller lokal".into());
    }
    let input = required(options, "--transskripter")?;
    let out = PathBuf::from(required(options, "--out")?);
    let mut files = fs::read_dir(input)
        .map_err(|_| "Kan ikke læse testmappe")?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    files.sort();
    if files.is_empty() {
        return Err("Ingen testfiler".into());
    }
    let mut records = Vec::new();
    for path in files {
        let temp = tempfile::tempdir().map_err(|_| "Kan ikke oprette midlertidig mappe")?;
        let result = temp.path().join("result.json");
        let log = temp.path().join("stderr.log");
        let mut command =
            Command::new(std::env::current_exe().map_err(|_| "Kan ikke finde programmet")?);
        for (key, value) in options {
            if !["--transskripter", "--out"].contains(&key.as_str()) {
                command.arg(key).arg(value);
            }
        }
        command
            .arg("--worker")
            .arg(&path)
            .arg("--result")
            .arg(&result)
            .stdout(Stdio::null())
            .stderr(fs::File::create(&log).map_err(|_| "Kan ikke oprette privat log")?);
        let mut child = command.spawn().map_err(|_| "Kan ikke starte måleproces")?;
        let pid = sysinfo::Pid::from_u32(child.id());
        let mut system = sysinfo::System::new();
        let mut process_peak = None;
        let mut helper_peak = None;
        let status = loop {
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            if let Some(process) = system.process(pid) {
                process_peak = Some(process_peak.unwrap_or(0).max(process.memory()));
            }
            let children = system
                .processes()
                .values()
                .filter(|p| {
                    p.parent() == Some(pid) && p.name().to_string_lossy().contains("llama-helper")
                })
                .collect::<Vec<_>>();
            if !children.is_empty() {
                helper_peak = Some(
                    helper_peak
                        .unwrap_or(0)
                        .max(children.iter().map(|p| p.memory()).sum::<u64>()),
                );
            }
            if let Some(status) = child.try_wait().map_err(|_| "Kan ikke vente på måling")? {
                break status;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        if !status.success() {
            return Err("Måleprocessen fejlede. Kontroller model, helper og argumenter; kildetekst er ikke skrevet til output.".into());
        }
        let mut record: Record =
            serde_json::from_slice(&fs::read(result).map_err(|_| "Måling mangler")?)
                .map_err(|_| "Ugyldig måling")?;
        record.process_peak_rss_bytes_sampled = process_peak;
        record.helper_peak_rss_bytes_sampled = helper_peak;
        helper_stats(
            &fs::read_to_string(log).map_err(|_| "Kan ikke læse helperstatistik")?,
            &mut record,
        );
        records.push(record);
    }
    fs::write(
        &out,
        serde_json::to_vec_pretty(&records).map_err(|_| "Kan ikke serialisere rapport")?,
    )
    .map_err(|_| "Kan ikke skrive rapport")?;
    let mut table = String::from("| Test | Minutter | Motor/model | Ønsket backend | Vulkan aktiv | Samlet s | Map s | Reduce s | Slut s | Oversættelse s | Proces RSS bytes | Helper RSS bytes | Tokens | Tokens/s | Estimerede tokens | Map/reduce-kald | Beslutninger | Opgaver | Mistænkte navne/tal |\n|---|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    fn cell<T: std::fmt::Display>(value: Option<T>) -> String {
        value
            .map(|n| n.to_string())
            .unwrap_or_else(|| "ikke målt".into())
    }
    for (i, r) in records.iter().enumerate() {
        let phase_seconds = |phase| {
            r.calls
                .iter()
                .filter(|c| c.phase == phase)
                .map(|c| c.seconds)
                .sum::<f64>()
        };
        table.push_str(&format!("| {} | {} | {}/{} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {} | {} | {} | {} | {} | {}/{} | {}/{} | {}/{} | {}/{} |\n",
            i + 1, r.minutes, r.engine, r.model, r.requested_backend, cell(r.vulkan_active), r.total_seconds,
            phase_seconds(map_reduce::Phase::Map), phase_seconds(map_reduce::Phase::Reduce),
            phase_seconds(map_reduce::Phase::Final), phase_seconds(map_reduce::Phase::Translation),
            cell(r.process_peak_rss_bytes_sampled), cell(r.helper_peak_rss_bytes_sampled),
            cell(r.generated_tokens), cell(r.tokens_per_second), r.estimated_output_tokens,
            r.stats.map_chunks, r.stats.reduce_calls, r.quality.decisions_found, r.quality.decisions_total,
            r.quality.tasks_found, r.quality.tasks_total, r.quality.suspect_names.len(), r.quality.suspect_numbers.len()));
    }
    fs::write(out.with_extension("md"), table).map_err(|_| "Kan ikke skrive tabel")?;
    println!(
        "Måling gemt: {} testfiler. Ingen kilde- eller referattekst i rapporten.",
        records.len()
    );
    Ok(())
}
#[tokio::main]
async fn main() {
    let result = async {
        let options = options()?;
        if let Some(dir) = options.get("--generer") {
            fs::create_dir_all(dir).map_err(|_| "Kan ikke oprette testmappe")?;
            for minutes in [2, 30, 90] {
                let fixture = eval::fixture(minutes);
                fs::write(
                    Path::new(dir).join(format!("{}.json", fixture.id)),
                    serde_json::to_vec_pretty(&fixture).map_err(|_| "Kan ikke serialisere test")?,
                )
                .map_err(|_| "Kan ikke gemme test")?;
            }
            Ok(())
        } else if options.contains_key("--worker") {
            worker(&options).await
        } else {
            coordinator(&options)
        }
    }
    .await;
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> Record {
        Record {
            id: "test".into(),
            minutes: 2,
            engine: "lokal".into(),
            model: "test".into(),
            requested_backend: "vulkan".into(),
            vulkan_active: None,
            total_seconds: 1.0,
            process_peak_rss_bytes_sampled: None,
            helper_peak_rss_bytes_sampled: None,
            generated_tokens: None,
            generation_seconds: None,
            tokens_per_second: None,
            estimated_output_tokens: 0,
            estimated_tokens_per_second_end_to_end: 0.0,
            stats: Default::default(),
            calls: vec![eval::Call {
                phase: map_reduce::Phase::Final,
                seconds: 1.0,
                estimated_output_tokens: 0,
            }],
            quality: eval::quality(&eval::fixture(2), ""),
        }
    }
    #[test]
    fn accepts_only_complete_numeric_helper_statistics_and_positive_offload() {
        let mut result = record();
        helper_stats(
            "• Output tokens: 100\n• Generation time: 2.00s\nVulkan device found\n",
            &mut result,
        );
        assert_eq!(result.generated_tokens, Some(100));
        assert_eq!(result.tokens_per_second, Some(50.0));
        assert_eq!(result.vulkan_active, None);
        helper_stats("Vulkan\noffloaded 12/32 layers\n", &mut result);
        assert_eq!(result.vulkan_active, Some(true));
        let mut missing = record();
        helper_stats(
            "• Output tokens: 100\n• Output tokens: 100\n• Generation time: 2.00s",
            &mut missing,
        );
        assert_eq!(missing.generated_tokens, None);
        helper_stats(
            "• Output tokens: 100\n• Generation time: 0.00s",
            &mut missing,
        );
        assert_eq!(missing.tokens_per_second, None);
    }
}
