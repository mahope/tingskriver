//! Headless transcription measurement for agents and CI. Installs no logger:
//! engine debug logs can contain transcripts. See docs/measurement.md.
use std::{path::PathBuf, process::exit};
use tingskriver_core::measurement::{bench, Sample};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", bench::USAGE);
        return;
    }
    let opts = match bench::parse_args(&args) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("{message}\n{}", bench::USAGE);
            exit(2);
        }
    };
    let models_dir = opts
        .models_dir
        .clone()
        .or_else(|| dirs::data_dir().map(|d| d.join("Tingskriver").join("models")))
        .unwrap_or_else(|| PathBuf::from("models"));
    if opts.list_models {
        let models = bench::list_models(&models_dir).await;
        if opts.json {
            let value: Vec<_> = models
                .iter()
                .map(|(n, e, s)| serde_json::json!({"model": n, "engine": e, "status": s}))
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
        } else {
            for (n, e, s) in models {
                println!("{n}\t{e}\t{s}");
            }
        }
        return;
    }
    let sample = match opts.file.as_deref().map(Sample::load) {
        Some(Ok(s)) => s,
        Some(Err(message)) => {
            eprintln!("{message}");
            exit(2);
        }
        None => unreachable!("parse_args requires --file unless --list-models"),
    };
    let reference = match &opts.reference {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(t) => Some(t),
            Err(_) => {
                eprintln!("Facit-filen kunne ikke læses.");
                exit(2);
            }
        },
        None => None,
    };
    let mut results = Vec::new();
    // One model at a time; each engine is dropped before the next is loaded.
    for model in &opts.models {
        results.push(
            bench::run_model(
                model,
                &models_dir,
                &sample,
                opts.repeat,
                reference.as_deref(),
            )
            .await,
        );
    }
    let code = bench::exit_code(&results);
    let report = bench::new_report(sample.duration_seconds(), opts.repeat, results);
    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        print!("{}", bench::render_text(&report));
    }
    if code != 0 {
        exit(code);
    }
}
