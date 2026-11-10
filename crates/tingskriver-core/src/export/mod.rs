//! Portable meeting documents. Load `MeetingDetails` with `MeetingsRepository`;
//! pass notes (via `notes::markdown::serialize`) and summary text explicitly.

use crate::api::MeetingDetails;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Metadata absent from the meeting model is supplied by the caller, never guessed.
pub struct ExportData<'a> {
    pub meeting: &'a MeetingDetails,
    pub duration_seconds: Option<u64>,
    /// Participant names or speaker labels, in display order.
    pub speakers: &'a [String],
    pub language: Option<&'a str>,
    pub summary: &'a str,
    pub notes: &'a str,
}

#[derive(Clone, Copy)]
pub enum Format {
    Markdown,
    Html,
}

/// Render and save one meeting document for a future UI wrapper.
/// `{dato}` uses the calendar date from the stored RFC 3339 creation timestamp.
pub async fn export_meeting(
    directory: &Path,
    template: &str,
    format: Format,
    data: &ExportData<'_>,
) -> io::Result<PathBuf> {
    let date = data
        .meeting
        .created_at
        .split('T')
        .next()
        .unwrap_or_default();
    let basename = filename(template, date, &data.meeting.title);
    let content = match format {
        Format::Markdown => render_markdown(data),
        Format::Html => render_html(data),
    };
    write_document(directory, &basename, format, content).await
}

// JSON strings and arrays are also YAML flow values. Quoting prevents dates,
// colons, newlines and document markers from changing the frontmatter structure.
fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

pub fn render_markdown(data: &ExportData<'_>) -> String {
    let duration = data
        .duration_seconds
        .map_or("null".into(), |n| n.to_string());
    let language = data.language.map_or("null".into(), quoted);
    let speakers = serde_json::to_string(data.speakers).expect("serializing strings cannot fail");
    let mut text = format!(
        "---\ntitle: {}\ndate: {}\nduration_seconds: {duration}\nspeakers: {speakers}\nlanguage: {language}\n---\n\n# {}\n\n## Referat\n\n{}\n\n## Noter\n\n{}\n\n## Transskript\n\n",
        quoted(&data.meeting.title), quoted(&data.meeting.created_at),
        data.meeting.title, data.summary, data.notes
    );
    for segment in ordered_transcripts(data.meeting) {
        text.push_str(&format!("[{}] {}\n\n", segment.timestamp, segment.text));
    }
    text
}

fn ordered_transcripts(meeting: &MeetingDetails) -> Vec<&crate::api::MeetingTranscript> {
    let mut segments: Vec<_> = meeting.transcripts.iter().collect();
    segments.sort_by(|a, b| match (a.audio_start_time, b.audio_start_time) {
        (Some(a), Some(b)) => a.total_cmp(&b),
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, None) => a.timestamp.cmp(&b.timestamp),
    });
    segments
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// A standalone, inert HTML copy. Summary and notes retain their source text;
/// raw Markdown/HTML is never interpreted as executable markup.
pub fn render_html(data: &ExportData<'_>) -> String {
    let title = escape_html(&data.meeting.title);
    let mut html = format!(
        "<!doctype html>\n<html lang=\"da\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title><style>body{{font-family:system-ui,sans-serif;max-width:70ch;margin:2em auto;padding:0 1em}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;font:inherit}}</style></head><body><h1>{title}</h1><dl>"
    );
    for (label, value) in [
        ("Dato", data.meeting.created_at.clone()),
        (
            "Varighed (sekunder)",
            data.duration_seconds
                .map_or(String::new(), |n| n.to_string()),
        ),
        ("Deltagere/talere", data.speakers.join(", ")),
        ("Sprog", data.language.unwrap_or_default().to_owned()),
    ] {
        html.push_str(&format!("<dt>{label}</dt><dd>{}</dd>", escape_html(&value)));
    }
    html.push_str("</dl>");
    for (label, value) in [("Referat", data.summary), ("Noter", data.notes)] {
        html.push_str(&format!(
            "<h2>{label}</h2><pre>{}</pre>",
            escape_html(value)
        ));
    }
    html.push_str("<h2>Transskript</h2>");
    for segment in ordered_transcripts(data.meeting) {
        html.push_str(&format!(
            "<pre>[{}] {}</pre>",
            escape_html(&segment.timestamp),
            escape_html(&segment.text)
        ));
    }
    html.push_str("</body></html>\n");
    html
}

/// Expand the two supported placeholders, then sanitize the entire basename.
pub fn filename(template: &str, date: &str, title: &str) -> String {
    // Split the template so placeholder-looking text in user data stays literal.
    let mut result = String::new();
    let mut rest = template;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("{dato}") {
            result.push_str(date);
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("{titel}") {
            result.push_str(title);
            rest = tail;
        } else {
            let ch = rest.chars().next().unwrap();
            result.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    sanitize_filename(&result)
}

pub fn sanitize_filename(value: &str) -> String {
    let mut name = String::new();
    let mut units = 0;
    for ch in value.chars() {
        let ch = if ch.is_control() || "<>:\"/\\|?*".contains(ch) {
            '_'
        } else {
            ch
        };
        // Leave room for the extension and collision suffix on Windows.
        if units + ch.len_utf16() > 180 || name.len() + ch.len_utf8() > 180 {
            break;
        }
        units += ch.len_utf16();
        name.push(ch);
    }
    name = name.trim_end_matches(['.', ' ']).to_owned();
    if name.is_empty() {
        return "meeting".into();
    }
    let stem = name.split('.').next().unwrap().trim_end().to_uppercase();
    let device_number = |prefix: &str| {
        stem.strip_prefix(prefix).is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    };
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || device_number("COM")
        || device_number("LPT")
    {
        name.insert(0, '_');
    }
    name
}

/// Write on the blocking pool, then atomically publish without replacing a file.
/// The destination directory must already exist. Returns the actual chosen path.
pub async fn write_document(
    directory: &Path,
    basename: &str,
    format: Format,
    content: String,
) -> io::Result<PathBuf> {
    let directory = directory.to_owned();
    let basename = sanitize_filename(basename);
    tokio::task::spawn_blocking(move || {
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
        temporary.write_all(content.as_bytes())?;
        temporary.as_file().sync_all()?;
        let extension = match format {
            Format::Markdown => "md",
            Format::Html => "html",
        };
        let mut number = 1u64;
        loop {
            let suffix = if number == 1 {
                String::new()
            } else {
                format!(" ({number})")
            };
            let path = directory.join(format!("{basename}{suffix}.{extension}"));
            // Unlike exists()+rename(), this remains safe against concurrent
            // writers. tempfile uses a no-replace rename or hard-link fallback.
            match temporary.persist_noclobber(&path) {
                Ok(_) => return Ok(path),
                Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                    temporary = error.file;
                    number = number
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("export suffix exhausted"))?;
                }
                Err(error) => return Err(error.error),
            }
        }
    })
    .await
    .map_err(io::Error::other)?
}

#[cfg(test)]
mod tests;
