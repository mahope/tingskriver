use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    /// `audio` or `fixture-text`; fixtures are never speech quality evidence.
    pub kind: String,
    pub clips: Vec<Clip>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub id: String,
    pub file: String,
    pub reference: String,
    pub speech_type: String,
    pub dialect: String,
    pub source: String,
    pub license: String,
    pub attribution: String,
    /// `documented`, `source-documented` or `not-applicable` (text fixtures).
    pub consent: String,
}

pub fn relative_file(root: &Path, name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("Invalid relative asset path");
    }
    let root = if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    };
    let root = root
        .canonicalize()
        .context("Cannot resolve asset directory")?;
    let file = root
        .join(path)
        .canonicalize()
        .context("Missing asset file")?;
    if !file.starts_with(root) || !file.is_file() {
        bail!("Asset must be a file inside the asset directory");
    }
    Ok(file)
}

/// Strict metadata and file validation; empty datasets cannot silently pass CI.
#[allow(clippy::disallowed_methods)] // Synchronous disk worker.
pub fn load(path: &Path) -> Result<Manifest> {
    let bytes = std::fs::read(path).context("Cannot read WER manifest")?;
    // Do not forward serde's error: it can contain reference text.
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid WER manifest schema"))?;
    if manifest.schema_version != 1 || !["audio", "fixture-text"].contains(&manifest.kind.as_str())
    {
        bail!("Unsupported WER manifest version or kind");
    }
    if manifest.clips.is_empty() || manifest.clips.len() > 20 {
        bail!("WER manifest must contain 1..20 clips");
    }
    let mut ids = HashSet::new();
    for clip in &manifest.clips {
        if clip.id.is_empty()
            || !clip
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            || !ids.insert(&clip.id)
        {
            bail!("Invalid or duplicate clip identifier");
        }
        if [
            &clip.file,
            &clip.reference,
            &clip.dialect,
            &clip.source,
            &clip.license,
            &clip.attribution,
        ]
        .iter()
        .any(|v| v.trim().is_empty())
            || !["oplaesning", "samtale"].contains(&clip.speech_type.as_str())
            || !["documented", "source-documented", "not-applicable"]
                .contains(&clip.consent.as_str())
            || (manifest.kind == "audio" && clip.consent == "not-applicable")
        {
            bail!("Missing or invalid clip metadata");
        }
        relative_file(path.parent().unwrap_or(Path::new(".")), &clip.file)?;
    }
    Ok(manifest)
}
