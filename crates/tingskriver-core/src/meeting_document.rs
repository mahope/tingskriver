//! The meeting document: what the meeting page shows for one meeting (Tauri
//! `MeetingDocument.tsx`). Title and time, the summary as read-only blocks,
//! and the transcript lines. UI-independent; the GPUI app renders it.
//!
//! The summary is the `markdown` field of the meeting's summary process, the
//! same field MCP and the proposal store read. Editing is not part of this
//! module (the read-only view comes first, docs/design-paritet.md Job 4).

use chrono::{DateTime, TimeZone, Utc};
use sqlx::SqlitePool;

use crate::database::repositories::{
    meeting::MeetingsRepository, summary::SummaryProcessesRepository,
};
use crate::format_da;

/// One meeting, ready to render.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    /// The recording folder; `None` for an imported transcript without audio.
    pub folder_path: Option<String>,
    pub summary: Summary,
    pub transcript: Vec<Line>,
}

/// Where the summary stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryStatus {
    /// No summary process, or it was cancelled.
    Idle,
    /// Being written now.
    Running,
    Completed,
    Failed,
}

/// The meeting's summary: its status, the Markdown (also kept after a failed
/// regeneration, which restores the previous result) and the error text.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub status: SummaryStatus,
    pub markdown: Option<String>,
    pub error: Option<String>,
}

impl Summary {
    pub fn none() -> Self {
        Summary {
            status: SummaryStatus::Idle,
            markdown: None,
            error: None,
        }
    }

    /// Whether there is visible summary text (Tauri `hasVisibleSummaryContent`).
    pub fn has_content(&self) -> bool {
        self.markdown
            .as_deref()
            .is_some_and(|m| !m.trim().is_empty())
    }
}

/// One transcript line: the recording-relative time and the text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Seconds from the start of the recording; `None` for old transcripts.
    pub start: Option<f64>,
    /// The wall-clock timestamp stored with old transcripts.
    pub timestamp: String,
    pub text: String,
}

impl Line {
    /// "00:03", "1:02:03", or "--:--" without a recording time
    /// (Tauri `formatRecordingTime`).
    pub fn time_label(&self) -> String {
        match self.start {
            Some(seconds) => recording_time(seconds),
            None => "--:--".into(),
        }
    }
}

/// Recording-relative time as MM:SS, or H:MM:SS from one hour.
pub fn recording_time(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds.floor() as u64
    } else {
        0
    };
    let (hours, minutes, secs) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

/// "29. okt. 2026 kl. 09.30" in the given time zone (Tauri `MeetingHeader`).
pub fn date_line<T: TimeZone>(created_at: DateTime<Utc>, timezone: &T) -> String {
    let local = created_at.with_timezone(timezone).naive_local();
    format!(
        "{} {}",
        format_da::short_date(local.date()),
        format_da::time(local.time())
    )
}

/// The summary status from the stored process status.
pub fn summary_status(stored: &str) -> SummaryStatus {
    match stored.to_ascii_lowercase().as_str() {
        "pending" | "processing" | "summarizing" | "regenerating" | "running" => {
            SummaryStatus::Running
        }
        "completed" => SummaryStatus::Completed,
        "failed" | "error" => SummaryStatus::Failed,
        _ => SummaryStatus::Idle,
    }
}

/// The Markdown of a stored summary result, read as the Tauri app reads it
/// (`api_get_summary` and `summary-content.ts`): reasoning removed, then the
/// `markdown` field, else the BlockNote blocks in `summary_json`, else the
/// old section format. `None` when nothing renderable is stored, also when
/// a reasoning marker is left over.
pub fn summary_markdown(result: Option<&str>) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(result?).ok()?;
    let value = crate::summary::commands::redact_flagged_reasoning(value);
    if !crate::summary::commands::summary_is_renderable(&value) {
        return None;
    }
    let object = value.as_object()?;
    let markdown = if let Some(markdown) = object.get("markdown") {
        markdown.as_str()?.to_string()
    } else if let Some(blocks) = object.get("summary_json") {
        let mut out = Vec::new();
        blocknote_markdown(blocks, 0, &mut out);
        out.join("\n")
    } else {
        legacy_markdown(object)
    };
    (!markdown.trim().is_empty()).then_some(markdown)
}

/// The summary from the stored process: like the Tauri app, a "completed"
/// process without renderable text counts as failed.
fn summary_from_process(status: &str, result: Option<&str>, error: Option<String>) -> Summary {
    let mut summary = Summary {
        status: summary_status(status),
        markdown: summary_markdown(result),
        error: error.filter(|e| !e.trim().is_empty()),
    };
    if summary.status == SummaryStatus::Completed && summary.markdown.is_none() {
        summary.status = SummaryStatus::Failed;
        summary.error = Some(crate::strings::DOC_SUMMARY_INVALID.to_string());
    }
    summary
}

/// BlockNote blocks as Markdown lines, nested blocks indented.
fn blocknote_markdown(value: &serde_json::Value, depth: usize, out: &mut Vec<String>) {
    let Some(blocks) = value.as_array() else {
        return;
    };
    let mut number = 0;
    for block in blocks {
        let kind = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let props = block.get("props");
        let text = block
            .get("content")
            .map(blocknote_inline)
            .unwrap_or_default();
        number = if kind == "numberedListItem" {
            number + 1
        } else {
            0
        };
        let indent = "  ".repeat(depth);
        let line = match kind {
            "heading" => {
                let level = props
                    .and_then(|p| p.get("level"))
                    .and_then(|l| l.as_u64())
                    .unwrap_or(1)
                    .clamp(1, 6) as usize;
                format!("{} {text}", "#".repeat(level))
            }
            "bulletListItem" => format!("{indent}- {text}"),
            "numberedListItem" => format!("{indent}{number}. {text}"),
            "checkListItem" => {
                let checked = props
                    .and_then(|p| p.get("checked"))
                    .and_then(|c| c.as_bool())
                    .unwrap_or(false);
                format!("{indent}- [{}] {text}", if checked { 'x' } else { ' ' })
            }
            "quote" => format!("> {text}"),
            _ => text,
        };
        if !line.trim().is_empty() {
            out.push(line);
        }
        if let Some(children) = block.get("children") {
            blocknote_markdown(children, depth + 1, out);
        }
    }
}

/// The inline content of a BlockNote block with bold, italic and code kept.
fn blocknote_inline(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items.iter().map(blocknote_inline).collect(),
        serde_json::Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(|t| t.as_str()) {
                let style = |name: &str| {
                    object
                        .get("styles")
                        .and_then(|s| s.get(name))
                        .and_then(|v| v.as_bool())
                        == Some(true)
                };
                let mut text = text.to_string();
                if text.trim().is_empty() {
                    return text;
                }
                if style("code") {
                    text = format!("`{text}`");
                }
                if style("italic") {
                    text = format!("*{text}*");
                }
                if style("bold") {
                    text = format!("**{text}**");
                }
                text
            } else if let Some(content) = object.get("content") {
                blocknote_inline(content)
            } else if let Some(rows) = object.get("rows") {
                // A table: one line of cells per row.
                rows.as_array()
                    .map(|rows| {
                        rows.iter()
                            .map(|row| {
                                row.get("cells")
                                    .and_then(|c| c.as_array())
                                    .map(|cells| {
                                        cells
                                            .iter()
                                            .map(blocknote_inline)
                                            .collect::<Vec<_>>()
                                            .join(" | ")
                                    })
                                    .unwrap_or_default()
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

/// The old section format as the Tauri "Kopiér" button converts it
/// (`useCopyOperations.ts`): `## title` and one bullet per block, in
/// `_section_order` when it is stored.
fn legacy_markdown(object: &serde_json::Map<String, serde_json::Value>) -> String {
    let skip = |key: &str| {
        matches!(
            key,
            "markdown" | "summary_json" | "_section_order" | "MeetingName"
        )
    };
    let mut keys: Vec<&str> = object
        .get("_section_order")
        .and_then(|o| o.as_array())
        .map(|order| order.iter().filter_map(|k| k.as_str()).collect())
        .unwrap_or_default();
    for key in object.keys() {
        if !skip(key) && !keys.contains(&key.as_str()) {
            keys.push(key);
        }
    }
    keys.into_iter()
        .filter_map(|key| {
            let section = object.get(key)?;
            let title = section.get("title")?.as_str()?;
            let blocks = section.get("blocks")?.as_array()?;
            let lines: Vec<String> = blocks
                .iter()
                .filter_map(|b| b.get("content")?.as_str())
                .filter(|c| !c.trim().is_empty())
                .map(|c| format!("- {c}"))
                .collect();
            (!lines.is_empty()).then(|| format!("## {title}\n\n{}", lines.join("\n")))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Read one meeting with its summary and transcript. `Ok(None)` when the
/// meeting does not exist (deleted while it was open).
pub async fn load(pool: &SqlitePool, id: &str) -> anyhow::Result<Option<Document>> {
    let Some(meeting) = MeetingsRepository::get_meeting_metadata(pool, id).await? else {
        return Ok(None);
    };
    let (rows, _) =
        MeetingsRepository::get_meeting_transcripts_paginated(pool, id, i64::MAX, 0).await?;
    let summary = match SummaryProcessesRepository::get_summary_data(pool, id).await? {
        Some(process) => {
            summary_from_process(&process.status, process.result.as_deref(), process.error)
        }
        None => Summary::none(),
    };
    Ok(Some(Document {
        id: meeting.id,
        title: meeting.title,
        created_at: meeting.created_at.0,
        folder_path: meeting.folder_path.filter(|p| !p.trim().is_empty()),
        summary,
        transcript: rows
            .into_iter()
            .map(|row| Line {
                start: row.audio_start_time,
                timestamp: row.timestamp,
                text: row.transcript,
            })
            .collect(),
    }))
}

/// The transcript as the Tauri "Kopiér" button copies it
/// (`useCopyOperations.ts`): a title, the date and `[MM:SS] text` lines.
pub fn transcript_for_copy<T: TimeZone>(document: &Document, timezone: &T) -> String {
    let date = document.created_at.with_timezone(timezone).date_naive();
    let lines = document
        .transcript
        .iter()
        .map(|line| {
            let time = match line.start {
                Some(seconds) => format!("[{}]", recording_time(seconds)),
                None => line.timestamp.clone(),
            };
            format!("{time} {}  ", line.text)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# Transskript af mødet: {}\n\n## Dato: {}\n\n{lines}",
        document.title,
        format_da::short_date(date)
    )
}

/// The summary as the Tauri "Kopiér" button copies it: a title, id, meeting
/// time and copy time, then the Markdown. `None` without summary text.
pub fn summary_for_copy<T: TimeZone>(
    document: &Document,
    now: DateTime<Utc>,
    timezone: &T,
) -> Option<String> {
    let markdown = document.summary.markdown.as_deref()?;
    if markdown.trim().is_empty() {
        return None;
    }
    let stamp = |at: DateTime<Utc>| {
        let local = at.with_timezone(timezone).naive_local();
        format!(
            "{} {}",
            format_da::short_date(local.date()),
            format_da::clock(local.time())
        )
    };
    Some(format!(
        "# Opsummering af mødet: {}\n\n**Mødets id:** {}\n**Dato:** {}\n**Kopieret:** {}\n\n---\n\n{markdown}",
        document.title,
        document.id,
        stamp(document.created_at),
        stamp(now)
    ))
}

/// A block of the rendered summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// `#` to `######`, clamped to 1..=3.
    Heading(u8),
    Paragraph,
    /// A bullet with its nesting depth (0 at the margin).
    Bullet(u8),
    /// A numbered item with its number and depth.
    Numbered(u32, u8),
    /// A task line (`- [ ]` / `- [x]`) with its depth.
    Task(bool, u8),
    Quote,
    Rule,
}

/// A run of text with one style.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
}

impl Block {
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// Summary Markdown as read-only blocks: headings, paragraphs (lines joined
/// until a blank line), bullet, numbered and task lists, quotes and rules,
/// with bold, italic and code spans. Links keep their text only; HTML is
/// shown as text. Anything else is a paragraph, so no text is ever lost.
pub fn summary_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let flush = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block {
                kind: BlockKind::Paragraph,
                spans: inline(&paragraph.join(" ")),
            });
            paragraph.clear();
        }
    };
    let mut in_code = false;
    for raw in markdown.lines() {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            flush(&mut paragraph, &mut blocks);
            in_code = !in_code;
            continue;
        }
        if in_code {
            blocks.push(Block {
                kind: BlockKind::Paragraph,
                spans: vec![Span {
                    text: line.to_string(),
                    code: true,
                    ..Span::default()
                }],
            });
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
            continue;
        }
        let depth = depth(line);
        if let Some(block) = structural(trimmed, depth) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(block);
        } else {
            paragraph.push(trimmed);
        }
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// List nesting from leading spaces (two per level, a tab counts as one).
fn depth(line: &str) -> u8 {
    let mut width = 0usize;
    for c in line.chars() {
        match c {
            ' ' => width += 1,
            '\t' => width += 2,
            _ => break,
        }
    }
    (width / 2).min(4) as u8
}

fn structural(line: &str, depth: u8) -> Option<Block> {
    let block = |kind, text: &str| {
        Some(Block {
            kind,
            spans: inline(text),
        })
    };
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) {
        if let Some(rest) = line[hashes..].strip_prefix(' ') {
            let text = rest.trim().trim_end_matches('#').trim_end();
            return block(BlockKind::Heading(hashes.min(3) as u8), text);
        }
    }
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() >= 3
        && ["-", "*", "_"]
            .iter()
            .any(|m| compact.chars().all(|c| c.to_string() == *m))
    {
        return Some(Block {
            kind: BlockKind::Rule,
            spans: Vec::new(),
        });
    }
    if let Some(rest) = line.strip_prefix('>') {
        return block(BlockKind::Quote, rest.trim_start());
    }
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            for (task, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
                if let Some(text) = rest.strip_prefix(task) {
                    return block(BlockKind::Task(done, depth), text);
                }
            }
            return block(BlockKind::Bullet(depth), rest.trim_start());
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digits) {
        let rest = &line[digits..];
        if let Some(text) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            let number = line[..digits].parse().unwrap_or(1);
            return block(BlockKind::Numbered(number, depth), text.trim_start());
        }
    }
    None
}

/// Bold (`**`, `__`), italic (`*`), code (`` ` ``) and links (`[text](url)`,
/// text kept) in one line. Unclosed markers stay as text.
fn inline(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut current = String::new();
    let (mut bold, mut italic) = (false, false);
    let push = |spans: &mut Vec<Span>, text: &mut String, bold, italic, code| {
        if text.is_empty() {
            return;
        }
        let text = std::mem::take(text);
        if let Some(last) = spans.last_mut() {
            if last.bold == bold && last.italic == italic && last.code == code {
                last.text.push_str(&text);
                return;
            }
        }
        spans.push(Span {
            text,
            bold,
            italic,
            code,
        });
    };
    let closes = |from: usize, marker: &[char]| -> bool {
        (from..chars.len()).any(|i| chars[i..].starts_with(marker))
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() {
            current.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '`' {
            if let Some(end) = (i + 1..chars.len()).find(|&j| chars[j] == '`') {
                push(&mut spans, &mut current, bold, italic, false);
                let mut code: String = chars[i + 1..end].iter().collect();
                push(&mut spans, &mut code, bold, italic, true);
                i = end + 1;
                continue;
            }
        }
        if c == '[' {
            if let Some(close) = (i + 1..chars.len()).find(|&j| chars[j] == ']') {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = (close + 2..chars.len()).find(|&j| chars[j] == ')') {
                        current.extend(&chars[i + 1..close]);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        let pair = [c, c];
        if (c == '*' || c == '_') && chars[i..].starts_with(&pair) {
            if bold || closes(i + 2, &pair) {
                push(&mut spans, &mut current, bold, italic, false);
                bold = !bold;
                i += 2;
                continue;
            }
        } else if c == '*' {
            let opens = chars.get(i + 1).is_some_and(|n| !n.is_whitespace());
            // A lone `*` closes; the halves of a `**` do not.
            let lone_star =
                |j: usize| chars[j] == '*' && chars.get(j + 1) != Some(&'*') && chars[j - 1] != '*';
            if italic || (opens && (i + 1..chars.len()).any(lone_star)) {
                push(&mut spans, &mut current, bold, italic, false);
                italic = !italic;
                i += 1;
                continue;
            }
        }
        current.push(c);
        i += 1;
    }
    push(&mut spans, &mut current, bold, italic, false);
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Span {
        Span {
            text: text.into(),
            ..Span::default()
        }
    }

    #[test]
    fn headings_lists_and_paragraphs_become_blocks() {
        let blocks = summary_blocks(
            "# Kort fortalt\nTeamet gennemgik status\nog blev enige.\n\n## Beslutninger\n- Lancering tirsdag\n  - Test først\n1. Første\n2) Anden\n- [ ] Send adgang\n- [x] Book lokale\n\n---\n> Citat",
        );
        let kinds: Vec<_> = blocks.iter().map(|b| b.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                BlockKind::Heading(1),
                BlockKind::Paragraph,
                BlockKind::Heading(2),
                BlockKind::Bullet(0),
                BlockKind::Bullet(1),
                BlockKind::Numbered(1, 0),
                BlockKind::Numbered(2, 0),
                BlockKind::Task(false, 0),
                BlockKind::Task(true, 0),
                BlockKind::Rule,
                BlockKind::Quote,
            ]
        );
        assert_eq!(blocks[1].text(), "Teamet gennemgik status og blev enige.");
        assert_eq!(blocks[4].text(), "Test først");
        assert_eq!(blocks[7].text(), "Send adgang");
    }

    #[test]
    fn deep_headings_are_clamped_and_hashes_without_space_are_text() {
        let blocks = summary_blocks("#### Detaljer\n#hashtag");
        assert_eq!(blocks[0].kind, BlockKind::Heading(3));
        assert_eq!(blocks[0].text(), "Detaljer");
        assert_eq!(blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(blocks[1].text(), "#hashtag");
    }

    #[test]
    fn inline_styles_are_spans_and_unclosed_markers_stay_text() {
        let blocks = summary_blocks(
            "**Maria** sender *nu* `kode` og [linket](https://example.invalid) 2 * 3",
        );
        assert_eq!(
            blocks[0].spans,
            vec![
                Span {
                    text: "Maria".into(),
                    bold: true,
                    ..Span::default()
                },
                plain(" sender "),
                Span {
                    text: "nu".into(),
                    italic: true,
                    ..Span::default()
                },
                plain(" "),
                Span {
                    text: "kode".into(),
                    code: true,
                    ..Span::default()
                },
                plain(" og linket 2 * 3"),
            ]
        );
        assert_eq!(summary_blocks("**åben")[0].text(), "**åben");
        assert_eq!(
            summary_blocks(r"\*ikke kursiv\*")[0].text(),
            "*ikke kursiv*"
        );
        // The first half of a `**` does not close an italic.
        assert_eq!(
            summary_blocks("*a **b** c*")[0].spans,
            vec![
                Span {
                    text: "a ".into(),
                    italic: true,
                    ..Span::default()
                },
                Span {
                    text: "b".into(),
                    bold: true,
                    italic: true,
                    ..Span::default()
                },
                Span {
                    text: " c".into(),
                    italic: true,
                    ..Span::default()
                },
            ]
        );
        assert_eq!(
            summary_blocks("*a **b** c")[0].spans,
            vec![
                plain("*a "),
                Span {
                    text: "b".into(),
                    bold: true,
                    ..Span::default()
                },
                plain(" c"),
            ]
        );
    }

    #[test]
    fn code_fences_keep_their_lines_and_no_text_is_lost() {
        let source = "Før\n```\n# ikke en overskrift\n```\nEfter";
        let blocks = summary_blocks(source);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].text(), "# ikke en overskrift");
        assert!(blocks[1].spans[0].code);
        assert!(summary_blocks("").is_empty());
        assert!(summary_blocks("\n\n  \n").is_empty());
    }

    #[test]
    fn recording_times_match_the_tauri_format() {
        assert_eq!(recording_time(3.9), "00:03");
        assert_eq!(recording_time(754.0), "12:34");
        assert_eq!(recording_time(3723.0), "1:02:03");
        assert_eq!(recording_time(f64::NAN), "00:00");
        assert_eq!(recording_time(-4.0), "00:00");
        let line = Line {
            start: None,
            timestamp: "09:30:01".into(),
            text: "x".into(),
        };
        assert_eq!(line.time_label(), "--:--");
    }

    #[test]
    fn status_and_markdown_come_from_the_summary_process() {
        assert_eq!(summary_status("PENDING"), SummaryStatus::Running);
        assert_eq!(summary_status("completed"), SummaryStatus::Completed);
        assert_eq!(summary_status("failed"), SummaryStatus::Failed);
        assert_eq!(summary_status("cancelled"), SummaryStatus::Idle);
        assert_eq!(
            summary_markdown(Some(r##"{"markdown":"# Kort fortalt"}"##)).as_deref(),
            Some("# Kort fortalt")
        );
        assert_eq!(summary_markdown(Some(r#"{"markdown":"  "}"#)), None);
        assert_eq!(summary_markdown(Some("ikke json")), None);
        assert_eq!(summary_markdown(None), None);
    }

    #[test]
    fn summary_markdown_reads_every_stored_format() {
        // Reasoning is removed before the text is shown.
        assert_eq!(
            summary_markdown(Some(
                r#"{"markdown":"<think>overvejer</think>\n- Lancering tirsdag"}"#
            ))
            .as_deref(),
            Some("- Lancering tirsdag")
        );
        // An unterminated reasoning marker is never shown.
        assert_eq!(
            summary_markdown(Some(r#"{"markdown":"<think>halvt svar"}"#)),
            None
        );
        // Only BlockNote blocks: an edit whose Markdown conversion failed.
        let blocknote = r#"{"summary_json":[
            {"type":"heading","props":{"level":2},"content":[{"type":"text","text":"Beslutninger","styles":{}}],"children":[]},
            {"type":"bulletListItem","props":{},"content":[{"type":"text","text":"Lancering","styles":{"bold":true}},{"type":"text","text":" tirsdag","styles":{}}],
             "children":[{"type":"bulletListItem","props":{},"content":[{"type":"text","text":"Test først","styles":{}}],"children":[]}]},
            {"type":"numberedListItem","props":{},"content":[{"type":"text","text":"Første","styles":{}}],"children":[]},
            {"type":"numberedListItem","props":{},"content":[{"type":"text","text":"Anden","styles":{}}],"children":[]},
            {"type":"checkListItem","props":{"checked":true},"content":[{"type":"text","text":"Book lokale","styles":{}}],"children":[]},
            {"type":"paragraph","props":{},"content":[{"type":"link","href":"https://example.invalid","content":[{"type":"text","text":"Se noten","styles":{"italic":true}}]}],"children":[]}
        ]}"#;
        assert_eq!(
            summary_markdown(Some(blocknote)).as_deref(),
            Some("## Beslutninger\n- **Lancering** tirsdag\n  - Test først\n1. Første\n2. Anden\n- [x] Book lokale\n*Se noten*")
        );
        // The old section format, in the stored section order.
        let legacy = r#"{"MeetingName":"Møde","_section_order":["b","a"],
            "a":{"title":"Handlepunkter","blocks":[{"content":"Send adgang"}]},
            "b":{"title":"Kort fortalt","blocks":[{"content":"Status"},{"content":" "}]}}"#;
        assert_eq!(
            summary_markdown(Some(legacy)).as_deref(),
            Some("## Kort fortalt\n\n- Status\n\n## Handlepunkter\n\n- Send adgang")
        );
    }

    #[test]
    fn completed_without_renderable_text_counts_as_failed() {
        let summary = summary_from_process("completed", Some(r#"{"markdown":""}"#), None);
        assert_eq!(summary.status, SummaryStatus::Failed);
        assert_eq!(
            summary.error.as_deref(),
            Some(crate::strings::DOC_SUMMARY_INVALID)
        );
        assert!(!summary.has_content());
        let running = summary_from_process("processing", None, Some(" ".into()));
        assert_eq!(running.status, SummaryStatus::Running);
        assert_eq!(running.error, None);
        let done = summary_from_process("completed", Some(r#"{"markdown":"Tekst"}"#), None);
        assert_eq!(done.status, SummaryStatus::Completed);
        assert_eq!(done.markdown.as_deref(), Some("Tekst"));
    }

    #[tokio::test]
    async fn load_reads_title_summary_and_ordered_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::database::manager::DatabaseManager::new_from_app_data_dir(dir.path())
            .await
            .unwrap();
        let pool = db.pool();
        let now = Utc::now();
        sqlx::query("INSERT INTO meetings (id,title,created_at,updated_at,folder_path) VALUES ('m','Møde',?,?,'')")
            .bind(now).bind(now).execute(pool).await.unwrap();
        for (id, text, start) in [("b", "Anden", 9.0), ("a", "Første", 1.5)] {
            sqlx::query("INSERT INTO transcripts (id,meeting_id,transcript,timestamp,audio_start_time) VALUES (?,'m',?,'',?)")
                .bind(id).bind(text).bind(start).execute(pool).await.unwrap();
        }
        let doc = load(pool, "m").await.unwrap().unwrap();
        assert_eq!(doc.title, "Møde");
        assert_eq!(doc.folder_path, None, "an empty folder is no folder");
        assert_eq!(doc.summary, Summary::none());
        let texts: Vec<_> = doc.transcript.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["Første", "Anden"]);

        sqlx::query("INSERT INTO summary_processes (meeting_id,status,created_at,updated_at,result,error) VALUES ('m','failed',?,?,?,'Modellen svarede ikke')")
            .bind(now).bind(now).bind(r##"{"markdown":"# Kort fortalt"}"##)
            .execute(pool).await.unwrap();
        let doc = load(pool, "m").await.unwrap().unwrap();
        assert_eq!(doc.summary.status, SummaryStatus::Failed);
        assert!(doc.summary.has_content(), "the previous summary is kept");
        assert_eq!(doc.summary.error.as_deref(), Some("Modellen svarede ikke"));
        assert_eq!(load(pool, "missing").await.unwrap(), None);
    }

    fn document() -> Document {
        Document {
            id: "m1".into(),
            title: "Ugentligt statusmøde".into(),
            created_at: DateTime::parse_from_rfc3339("2026-10-29T09:30:00Z")
                .unwrap()
                .to_utc(),
            folder_path: None,
            summary: Summary {
                status: SummaryStatus::Completed,
                markdown: Some("# Kort fortalt".into()),
                error: None,
            },
            transcript: vec![
                Line {
                    start: Some(3.2),
                    timestamp: String::new(),
                    text: "Godmorgen".into(),
                },
                Line {
                    start: None,
                    timestamp: "09:31".into(),
                    text: "Gammel linje".into(),
                },
            ],
        }
    }

    #[test]
    fn date_line_and_copies_follow_the_tauri_text() {
        let doc = document();
        assert_eq!(date_line(doc.created_at, &Utc), "29. okt. 2026 kl. 09.30");
        assert_eq!(
            transcript_for_copy(&doc, &Utc),
            "# Transskript af mødet: Ugentligt statusmøde\n\n## Dato: 29. okt. 2026\n\n[00:03] Godmorgen  \n09:31 Gammel linje  "
        );
        let now = DateTime::parse_from_rfc3339("2026-10-30T08:05:00Z")
            .unwrap()
            .to_utc();
        assert_eq!(
            summary_for_copy(&doc, now, &Utc).unwrap(),
            "# Opsummering af mødet: Ugentligt statusmøde\n\n**Mødets id:** m1\n**Dato:** 29. okt. 2026 09.30\n**Kopieret:** 30. okt. 2026 08.05\n\n---\n\n# Kort fortalt"
        );
        let mut empty = doc;
        empty.summary = Summary::none();
        assert!(!empty.summary.has_content());
        assert_eq!(summary_for_copy(&empty, now, &Utc), None);
    }
}
