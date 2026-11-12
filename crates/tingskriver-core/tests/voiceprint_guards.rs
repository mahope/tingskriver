//! Source-level guards for voiceprints (issue #198, GDPR article 9).
//!
//! Voiceprints are biometric data and must stay on this PC. These tests fail
//! if network code, serialisation, logging of values or a new route to the
//! voiceprint module (export, delivery, MCP, diagnostics, ...) is added.
//! They read source text, so they run on every platform.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<crate>")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs" || e == "sql") {
            out.push(path);
        }
    }
}

/// Source without whole-line comments (docs may name what is forbidden).
fn code_of(source: &str) -> String {
    source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn voiceprint_sources() -> Vec<(String, String)> {
    let dir = repo_root().join("crates/tingskriver-core/src/voiceprint");
    let mut files = Vec::new();
    rust_files(&dir, &mut files);
    assert!(
        files.len() >= 6,
        "voiceprint module not found at {}",
        dir.display()
    );
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read_to_string(&p).unwrap())
        })
        .collect()
}

const NETWORK: &[&str] = &[
    "reqwest",
    "hyper",
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "tokio::net",
    "std::net",
    "ureq",
    "crate::http",
    "http::",
    "http://",
    "https://",
    "websocket",
    "tungstenite",
    "Command::new",
];

fn network_hits(code: &str) -> Vec<&'static str> {
    NETWORK
        .iter()
        .copied()
        .filter(|t| code.contains(t))
        .collect()
}

#[test]
fn voiceprint_code_has_no_network_access() {
    for (file, source) in voiceprint_sources() {
        let hits = network_hits(&code_of(&source));
        assert!(
            hits.is_empty(),
            "{file} touches the network or a process: {hits:?}"
        );
    }
}

#[test]
fn network_scanner_positive_control() {
    assert!(!network_hits("let c = reqwest::Client::new();").is_empty());
    assert!(!network_hits("use tokio::net::TcpStream;").is_empty());
    assert!(network_hits("let x = 1;").is_empty());
}

#[test]
fn voiceprint_code_cannot_serialise_anything() {
    for (file, source) in voiceprint_sources() {
        let code = code_of(&source);
        for token in [
            "Serialize",
            "Deserialize",
            "serde",
            "impl fmt::Display for Embedding",
            "impl Display for Embedding",
        ] {
            assert!(
                !code.contains(token),
                "{file} contains {token}: a voiceprint could be exported"
            );
        }
    }
}

/// Everything outside the string text of a log statement: inline `{name}`
/// arguments and the argument list after the format string.
fn log_arguments(code: &str) -> Vec<String> {
    let inline = Regex::new(r"\{([^}:]*)[^}]*\}").unwrap();
    code.split("log::")
        .skip(1)
        .filter_map(|statement| {
            let statement = statement.split(';').next()?;
            let open = statement.find('"')?;
            let rest = &statement[open + 1..];
            let mut close = None;
            let mut escaped = false;
            for (i, c) in rest.char_indices() {
                match (escaped, c) {
                    (true, _) => escaped = false,
                    (false, '\\') => escaped = true,
                    (false, '"') => {
                        close = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            let literal = &rest[..close?];
            let mut args: Vec<String> = inline
                .captures_iter(literal)
                .map(|c| c[1].to_string())
                .collect();
            args.push(rest[close? + 1..].to_string());
            Some(args.join(" "))
        })
        .collect()
}

const SENSITIVE: &str = r"(?i)embedding|values|score|target|encode|person|name|consent|exemplar|speaker|sample|note|\bid\b|text|secret";

#[test]
fn voiceprint_log_lines_carry_counts_only() {
    let sensitive = Regex::new(SENSITIVE).unwrap();
    let mut seen = 0;
    for (file, source) in voiceprint_sources() {
        if file == "tests.rs" {
            continue;
        }
        for args in log_arguments(&code_of(&source)) {
            seen += 1;
            assert!(
                !sensitive.is_match(&args),
                "{file}: a log line may print voiceprint data: {args}"
            );
        }
    }
    assert!(seen >= 5, "the scanner found only {seen} log statements");
}

#[test]
fn log_scanner_positive_control() {
    let sensitive = Regex::new(SENSITIVE).unwrap();
    let bad = [
        r#"log::info!("matched {person_name}");"#,
        r#"log::debug!("vector {:?}", embedding);"#,
        r#"log::info!("score", score);"#,
    ];
    for code in bad {
        let args = log_arguments(code);
        assert!(
            args.iter().any(|a| sensitive.is_match(a)),
            "not caught: {code}"
        );
    }
    let good = log_arguments(r#"log::info!("removed {removed} voiceprints ({} days)", days);"#);
    assert!(good.iter().all(|a| !sensitive.is_match(a)));
}

/// Only these files may mention voiceprints. Anything else (export,
/// delivery, MCP, diagnostics, logging, the database layer, the host) is a
/// new route to biometric data and must be reviewed before it is allowed.
const ALLOWED: &[&str] = &[
    "crates/tingskriver-core/src/voiceprint/",
    "crates/tingskriver-core/src/lib.rs",
    "crates/tingskriver-core/src/strings/mod.rs",
    "crates/tingskriver-core/src/install/mod.rs",
    "crates/tingskriver-core/migrations/20261105130000_add_voiceprint_consent.sql",
    "crates/tingskriver-app/src/pages/settings.rs",
    "crates/tingskriver-app/src/lib.rs",
];

fn unexpected_mentions(root: &Path, files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .filter_map(|path| {
            let relative = path
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let source = fs::read_to_string(path).ok()?;
            let mentions = source.to_lowercase().contains("voiceprint");
            let allowed = ALLOWED.iter().any(|a| relative.starts_with(a));
            (mentions && !allowed).then_some(relative)
        })
        .collect()
}

#[test]
fn nothing_else_reaches_the_voiceprint_module() {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in fs::read_dir(root.join("crates")).unwrap().flatten() {
        rust_files(&dir.path().join("src"), &mut files);
        rust_files(&dir.path().join("migrations"), &mut files);
    }
    rust_files(&root.join("frontend/src-tauri/src"), &mut files);
    assert!(files.len() > 100, "scanned only {} files", files.len());
    let found = unexpected_mentions(&root, &files);
    assert!(
        found.is_empty(),
        "unreviewed route to voiceprints: {found:?}"
    );
}

#[test]
fn route_scanner_positive_control() {
    let dir = tempfile::tempdir().unwrap();
    let leak = dir.path().join("crates/x/src/delivery.rs");
    fs::create_dir_all(leak.parent().unwrap()).unwrap();
    fs::write(&leak, "use crate::voiceprint::store;").unwrap();
    let found = unexpected_mentions(dir.path(), &[leak]);
    assert_eq!(found, vec!["crates/x/src/delivery.rs".to_string()]);
}

#[test]
fn the_database_migration_stores_no_biometrics() {
    let path = repo_root()
        .join("crates/tingskriver-core/migrations/20261105130000_add_voiceprint_consent.sql");
    let sql = fs::read_to_string(path).unwrap();
    let code: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    for token in ["blob", "embedding", "vector", "audio", "sample"] {
        assert!(!code.contains(token), "migration has a {token} column");
    }
    // Off by default and 90 days by default, in the schema as well.
    assert!(code.contains("enabled integer not null default 0"));
    assert!(code.contains("retention_days integer not null default 90"));
}

#[test]
fn uninstalling_with_delete_data_also_removes_voiceprints() {
    // user_data::remove_credentials deletes everything under this prefix.
    assert!(tingskriver_core::voiceprint::store::TARGET_PREFIX
        .starts_with(tingskriver_core::install::user_data::CREDENTIAL_PREFIX));
}

#[test]
fn the_settings_page_explains_voiceprints_in_both_languages() {
    let ui = fs::read_to_string(repo_root().join("crates/tingskriver-app/src/pages/settings.rs"))
        .unwrap();
    for needle in [
        "strings::VOICEPRINT_TITLE",
        "strings::VOICEPRINT_STATUS_OFF",
        "voiceprint::texts",
        "texts::EN_STATUS_OFF",
        "p.da",
        "p.en",
    ] {
        assert!(ui.contains(needle), "settings page lacks {needle}");
    }
}
