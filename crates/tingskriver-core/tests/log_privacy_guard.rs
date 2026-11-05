//! Guard for #191: log calls must not carry personal data.
//!
//! The log file masks e-mail addresses, IP addresses, UUIDs, keys and
//! profile folders (`logging::Redactor`), but free text (meeting titles,
//! participant names, transcript text, calendar titles, folder names that
//! are built from a title) cannot be recognized by pattern. This test reads
//! the sources of the places that handle such data and fails when a log
//! macro receives one of those values as an argument.
//!
//! Logging that a value exists (`title.is_some()`, `transcripts.len()`) is
//! fine; logging the value is not. Log ids, counts, status and error
//! messages instead. If a new log call fails here, remove the value from it.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Source folders and files that handle meeting, calendar, participant,
/// provider and delivery data (relative to this crate).
const GUARDED: &[&str] = &[
    "src/api",
    "src/calendar",
    "src/summary",
    "src/providers",
    "src/delivery",
    "src/dictation",
    "src/notes",
    "src/database",
    "src/audio/import.rs",
    "src/audio/recording_commands.rs",
    "src/audio/recording_preferences.rs",
    "src/whisper_engine/commands.rs",
    "src/parakeet_engine/commands.rs",
];

const MACROS: &str = r"\b(?:log::)?(?:log_)?(?:info|warn|error|debug|trace)!\s*\(";

/// Values that are free text or can hold it, as identifiers in the
/// arguments of a log macro.
const FORBIDDEN: &str = r"(?i)\b(title|meeting_title|meeting_name|attendees?|participants?|organizer|emails?|ics\w*|address|webhook\w*|endpoint|api_key|secret|password|transcript_text|summary_text|speaker_name|folder_path|source_path|file_path|response_text|body|prompt|content)\b";

fn rust_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    for entry in fs::read_dir(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().unwrap().to_string_lossy();
            if name != "tests.rs" && !name.ends_with("_tests.rs") {
                out.push(path);
            }
        }
    }
}

/// The text between the parentheses of a macro call starting at `start`
/// (just after the opening one).
fn call_body(src: &str, start: usize) -> &str {
    let bytes = src.as_bytes();
    let (mut depth, mut i) = (1usize, start);
    while depth > 0 && i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    &src[start..i.saturating_sub(1).max(start)]
}

/// The first string literal is the format; the rest are the arguments.
/// Returns the identifiers that would be printed: arguments after the
/// format string and `{name}` captures inside it. Presence checks
/// (`x.is_some()`, `x.len()`) print no value and are dropped.
fn printed_identifiers(body: &str) -> String {
    let literal = Regex::new(r#"(?s)"(?:[^"\\]|\\.)*""#).unwrap();
    let presence = Regex::new(r"[\w.]+\.(?:is_some|is_none|is_empty|len)\(\)").unwrap();
    let capture = Regex::new(r"\{(\w+)(?::[^}]*)?\}").unwrap();
    let first = literal.find(body);
    let format = first.map(|m| m.as_str()).unwrap_or("");
    let rest = first.map(|m| &body[m.end()..]).unwrap_or(body);
    let rest = literal.replace_all(rest, "");
    let rest = presence.replace_all(&rest, "");
    let mut out = rest.into_owned();
    for c in capture.captures_iter(format) {
        out.push(' ');
        out.push_str(&c[1]);
    }
    out
}

fn violations(src: &str) -> Vec<String> {
    let macros = Regex::new(MACROS).unwrap();
    let forbidden = Regex::new(FORBIDDEN).unwrap();
    macros
        .find_iter(src)
        .filter_map(|m| {
            let body = call_body(src, m.end());
            let printed = printed_identifiers(body);
            forbidden.find(&printed).map(|hit| {
                let line = src[..m.start()].matches('\n').count() + 1;
                format!(
                    "line {line}: `{}` is logged in `{}`",
                    hit.as_str(),
                    body.split_whitespace().collect::<Vec<_>>().join(" ")
                )
            })
        })
        .collect()
}

#[test]
fn guarded_sources_log_no_personal_values() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for rel in GUARDED {
        rust_files(&root.join(rel), &mut files);
    }
    assert!(files.len() > 20, "the guard found only {} files", files.len());
    let mut found = Vec::new();
    for file in files {
        let src = fs::read_to_string(&file).unwrap();
        for v in violations(&src) {
            found.push(format!("{}: {v}", file.strip_prefix(root).unwrap().display()));
        }
    }
    assert!(
        found.is_empty(),
        "log calls with personal values (log ids, counts or status instead):\n{}",
        found.join("\n")
    );
}

/// The guard itself must be able to fail (#191).
#[test]
fn the_guard_flags_known_bad_calls_and_accepts_good_ones() {
    let bad = [
        r#"info!("Fetched {}", meeting_details.title);"#,
        r#"log::warn!("Folder {}", folder_path);"#,
        r#"error!("Failed for '{}': {}", meeting_title, e);"#,
        r#"info!("Invited {attendees:?}");"#,
        r#"log_info!("Using {}", config.endpoint);"#,
        r#"warn!("Response: {}", response_text);"#,
    ];
    for call in bad {
        assert_eq!(violations(call).len(), 1, "not flagged: {call}");
    }
    let good = [
        r#"info!("Fetched meeting title");"#,
        r#"info!("named meeting: {}", meeting_name.is_some());"#,
        r#"info!("transcripts: {}", transcripts.len());"#,
        r#"error!("Failed to save meeting {}: {}", meeting_id, e);"#,
    ];
    for call in good {
        assert!(violations(call).is_empty(), "wrongly flagged: {call}");
    }
}
