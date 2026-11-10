//! Intentionally installs no logger: engine debug logs can contain transcripts.
use anyhow::{bail, Context, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tingskriver_core::wer::{
    manifest,
    runner::{self, Engine, FixtureEngine, ModelEngine},
};

#[tokio::main]
async fn main() {
    if execute().await.is_err() {
        // Never forward arbitrary engine/serde errors or reference/hypothesis text.
        eprintln!("WER-kørsel fejlede. Kontrollér argumenter, manifest, filer og installerede modeller; se docs/wer.md.");
        std::process::exit(2);
    }
}

async fn execute() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut options = HashMap::new();
    while let Some(key) = args.next() {
        if ![
            "--manifest",
            "--engine",
            "--out",
            "--hypotheses",
            "--models-dir",
            "--model",
        ]
        .contains(&key.as_str())
            || options.contains_key(&key)
        {
            bail!("Invalid option");
        }
        options.insert(key, args.next().context("Missing option value")?);
    }
    let required = |key: &str| options.get(key).cloned().context("Missing required option");
    let path = PathBuf::from(required("--manifest")?);
    let manifest = manifest::load(&path)?;
    let engine_name = required("--engine")?;
    let engine: Box<dyn Engine> = if engine_name == "fixture" {
        Box::new(FixtureEngine {
            hypotheses: PathBuf::from(required("--hypotheses")?),
        })
    } else {
        Box::new(
            ModelEngine::load(
                &engine_name,
                PathBuf::from(required("--models-dir")?),
                required("--model")?,
            )
            .await?,
        )
    };
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .context("Cannot resolve git SHA")?;
    if !sha.status.success() {
        bail!("Cannot resolve git SHA");
    }
    let sha = String::from_utf8(sha.stdout)
        .context("Invalid git SHA")?
        .trim()
        .to_owned();
    let report = runner::run(
        &manifest,
        path.parent().unwrap_or(Path::new(".")),
        engine.as_ref(),
        sha,
    )
    .await?;
    std::fs::write(required("--out")?, serde_json::to_vec_pretty(&report)?)
        .context("Cannot write WER report")?;
    println!("WER-rapport skrevet ({} klip).", report.clips.len());
    Ok(())
}
