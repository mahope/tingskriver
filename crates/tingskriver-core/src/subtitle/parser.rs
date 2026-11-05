//! Pure parser for WebVTT (`.vtt`) and SubRip (`.srt`) subtitle files.
//!
//! Handles the variants written by Teams, Zoom and Meet: a byte order mark,
//! CRLF line endings, cue identifiers, `.` or `,` as the decimal separator,
//! timestamps with or without hours, cue settings after the end time,
//! multi-line cue text, WebVTT voice tags (`<v Navn>`) and Zoom's
//! `Navn: tekst` convention. Malformed blocks are skipped and counted; a file
//! with no usable cue at all is an error.

use std::fmt;

/// Which of the two formats a file turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    WebVtt,
    SubRip,
}

/// One timed piece of the transcript. Times are milliseconds from the start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSubtitles {
    pub format: SubtitleFormat,
    pub cues: Vec<Cue>,
    /// Blocks with a timing line or text that could not be used.
    pub skipped_blocks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleError {
    Empty,
    NoCues,
}

impl fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubtitleError::Empty => write!(f, "The file is empty"),
            SubtitleError::NoCues => write!(f, "The file contains no readable subtitle cues"),
        }
    }
}

impl std::error::Error for SubtitleError {}

/// Longest speaker label accepted by the `Navn: tekst` heuristic.
const MAX_INLINE_SPEAKER_CHARS: usize = 40;

pub fn parse_subtitles(content: &str) -> Result<ParsedSubtitles, SubtitleError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    if content.trim().is_empty() {
        return Err(SubtitleError::Empty);
    }
    let normalised = content.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalised.split('\n').collect();

    let first = lines.first().map(|l| l.trim_end()).unwrap_or("");
    let is_vtt = first == "WEBVTT" || first.starts_with("WEBVTT ") || first.starts_with("WEBVTT\t");
    let format = if is_vtt {
        SubtitleFormat::WebVtt
    } else {
        SubtitleFormat::SubRip
    };

    let mut cues = Vec::new();
    let mut skipped = 0usize;
    let mut i = if is_vtt { 1 } else { 0 };

    while i < lines.len() {
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }
        // Collect one block: consecutive non-blank lines.
        let start = i;
        while i < lines.len() && !lines[i].trim().is_empty() {
            i += 1;
        }
        let block = &lines[start..i];

        let timing_idx = block.iter().position(|l| l.contains("-->"));
        let Some(timing_idx) = timing_idx else {
            // NOTE/STYLE/REGION blocks and stray identifiers are not cues.
            let head = block[0].trim();
            let is_meta = head == "STYLE"
                || head == "REGION"
                || head.starts_with("NOTE")
                || head.starts_with("STYLE ")
                || head.starts_with("REGION ");
            if !is_meta {
                skipped += 1;
            }
            continue;
        };

        let Some((start_ms, end_ms)) = parse_timing_line(block[timing_idx]) else {
            skipped += 1;
            continue;
        };
        if end_ms < start_ms {
            skipped += 1;
            continue;
        }

        let raw_text = block[timing_idx + 1..].join(" ");
        let pieces = split_voices(&raw_text);
        if pieces.is_empty() {
            // A cue with no text (for example an empty Teams cue) is dropped.
            continue;
        }
        for (speaker, text) in pieces {
            cues.push(Cue {
                start_ms,
                end_ms,
                speaker,
                text,
            });
        }
    }

    if cues.is_empty() {
        return Err(SubtitleError::NoCues);
    }

    apply_inline_speaker_convention(&mut cues);

    Ok(ParsedSubtitles {
        format,
        cues,
        skipped_blocks: skipped,
    })
}

fn parse_timing_line(line: &str) -> Option<(u64, u64)> {
    let (left, right) = line.split_once("-->")?;
    let start = parse_timestamp(left.trim())?;
    // Cue settings (`align:start position:0%`) may follow the end time.
    let end = parse_timestamp(right.split_whitespace().next()?)?;
    Some((start, end))
}

/// `[HH:]MM:SS[.,]fff`, fraction of 1 to 3 digits.
fn parse_timestamp(s: &str) -> Option<u64> {
    let (clock, frac) = match s.find(['.', ',']) {
        Some(p) => (&s[..p], &s[p + 1..]),
        None => (s, ""),
    };
    if frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut ms = 0u64;
    if !frac.is_empty() {
        let padded = format!("{:0<3}", frac);
        ms = padded.parse().ok()?;
    }
    let parts: Vec<&str> = clock.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [m, s] => ("0", *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    let num = |p: &str| -> Option<u64> {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || p.len() > 6 {
            None
        } else {
            p.parse().ok()
        }
    };
    let (h, m, sec) = (num(h)?, num(m)?, num(sec)?);
    if m >= 60 || sec >= 60 {
        return None;
    }
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

/// Splits cue text into `(speaker, text)` pieces. Voice tags switch speaker;
/// every other tag is dropped; entities are decoded last.
fn split_voices(raw: &str) -> Vec<(Option<String>, String)> {
    let mut pieces: Vec<(Option<String>, String)> = Vec::new();
    let mut speaker: Option<String> = None;
    let mut buf = String::new();

    let flush =
        |speaker: &Option<String>, buf: &mut String, pieces: &mut Vec<(Option<String>, String)>| {
            let text = decode_entities(&collapse_ws(buf));
            buf.clear();
            if !text.is_empty() {
                pieces.push((speaker.clone(), text));
            }
        };

    let mut rest = raw;
    while let Some(lt) = rest.find('<') {
        buf.push_str(&rest[..lt]);
        let after = &rest[lt + 1..];
        let tag_like = after
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '/');
        match after
            .find('>')
            .filter(|gt| tag_like && !after[..*gt].contains('<'))
        {
            Some(gt) => {
                let tag = &after[..gt];
                if let Some(name) = voice_name(tag) {
                    flush(&speaker, &mut buf, &mut pieces);
                    speaker = if name.is_empty() {
                        None
                    } else {
                        Some(decode_entities(&name))
                    };
                } else if tag.trim() == "/v" {
                    flush(&speaker, &mut buf, &mut pieces);
                    speaker = None;
                }
                rest = &after[gt + 1..];
            }
            None => {
                // A lone `<` is ordinary text.
                buf.push('<');
                rest = after;
            }
        }
    }
    buf.push_str(rest);
    flush(&speaker, &mut buf, &mut pieces);
    pieces
}

/// `v Navn`, `v.klasse Navn` -> `Navn`. Returns None for other tags.
fn voice_name(tag: &str) -> Option<String> {
    let t = tag.trim_start();
    let rest = t.strip_prefix('v')?;
    match rest.chars().next() {
        None => Some(String::new()),
        Some(c) if c == '.' || c.is_whitespace() => {
            // Drop the optional `.class` part up to the first whitespace.
            let name = rest.trim_start_matches(|c: char| c != ' ' && c != '\t');
            Some(collapse_ws(name))
        }
        _ => None,
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lrm;", "")
        .replace("&rlm;", "")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Zoom writes `Navn: tekst` inside the cue. Treated as a speaker only when
/// most cues in the file follow the pattern, so a stray colon in ordinary
/// speech never invents a speaker.
fn apply_inline_speaker_convention(cues: &mut [Cue]) {
    if cues.iter().any(|c| c.speaker.is_some()) {
        return;
    }
    let split: Vec<Option<(String, String)>> =
        cues.iter().map(|c| split_inline_speaker(&c.text)).collect();
    let matches = split.iter().filter(|s| s.is_some()).count();
    if matches < 2 || matches * 10 < cues.len() * 6 {
        return;
    }
    // Cues without a prefix continue the previous speaker (Zoom wraps long turns).
    let mut current: Option<String> = None;
    for (cue, s) in cues.iter_mut().zip(split) {
        match s {
            Some((name, text)) => {
                current = Some(name.clone());
                cue.speaker = Some(name);
                cue.text = text;
            }
            None => cue.speaker = current.clone(),
        }
    }
}

fn split_inline_speaker(text: &str) -> Option<(String, String)> {
    let (name, rest) = text.split_once(": ")?;
    let name = name.trim();
    let rest = rest.trim();
    if name.is_empty()
        || rest.is_empty()
        || name.chars().count() > MAX_INLINE_SPEAKER_CHARS
        || name
            .chars()
            .any(|c| matches!(c, '.' | '?' | '!' | ',' | ';' | ':' | '(' | ')' | '"'))
        || name.chars().all(|c| c.is_ascii_digit() || c == ' ')
    {
        return None;
    }
    Some((name.to_string(), rest.to_string()))
}

/// Transcript as plain text for the summary prompt: consecutive cues from the
/// same speaker are joined under one `Navn: ` prefix.
pub fn cues_to_prompt_text(cues: &[Cue]) -> String {
    let mut out = String::new();
    let mut last: Option<&Option<String>> = None;
    for cue in cues {
        if last == Some(&cue.speaker) {
            out.push(' ');
            out.push_str(&cue.text);
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            if let Some(name) = &cue.speaker {
                out.push_str(name);
                out.push_str(": ");
            }
            out.push_str(&cue.text);
            last = Some(&cue.speaker);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // All fixtures are synthetic.
    const TEAMS_VTT: &str = "WEBVTT\r\n\r\n\
        5c1f0a2e-1111-4222-8333-444455556666/10-0\r\n\
        00:00:01.500 --> 00:00:04.000\r\n\
        <v Anna Testesen>Goddag og velkommen.</v>\r\n\r\n\
        5c1f0a2e-1111-4222-8333-444455556666/11-0\r\n\
        00:00:04.250 --> 00:00:07.000\r\n\
        <v Bo Eksempel>Tak, vi starter med &amp; punkt et\r\nog to.</v>\r\n";

    #[test]
    fn teams_vtt_gives_speakers_and_timecodes() {
        let p = parse_subtitles(TEAMS_VTT).unwrap();
        assert_eq!(p.format, SubtitleFormat::WebVtt);
        assert_eq!(p.cues.len(), 2);
        assert_eq!(p.cues[0].speaker.as_deref(), Some("Anna Testesen"));
        assert_eq!((p.cues[0].start_ms, p.cues[0].end_ms), (1500, 4000));
        assert_eq!(p.cues[0].text, "Goddag og velkommen.");
        assert_eq!(p.cues[1].speaker.as_deref(), Some("Bo Eksempel"));
        assert_eq!(p.cues[1].start_ms, 4250);
        assert_eq!(p.cues[1].text, "Tak, vi starter med & punkt et og to.");
        assert_eq!(p.skipped_blocks, 0);
    }

    #[test]
    fn srt_without_speakers() {
        let srt = "1\n00:00:00,000 --> 00:00:02,500\nFørste linje\nanden linje\n\n2\n00:01:02,5 --> 00:01:05,000\nÆbler og øl\n";
        let p = parse_subtitles(srt).unwrap();
        assert_eq!(p.format, SubtitleFormat::SubRip);
        assert_eq!(p.cues.len(), 2);
        assert!(p.cues.iter().all(|c| c.speaker.is_none()));
        assert_eq!(p.cues[0].text, "Første linje anden linje");
        assert_eq!(p.cues[1].start_ms, 62_500);
        assert_eq!(p.cues[1].text, "Æbler og øl");
    }

    #[test]
    fn bom_and_missing_hours_and_cue_settings() {
        let vtt = "\u{feff}WEBVTT - titel\n\nNOTE en kommentar\nover to linjer\n\nSTYLE\n::cue { color: red }\n\n01:02.000 --> 01:03.500 align:start position:0%\n<c.yellow><i>Hej</i></c> <00:01:02.500>med dig\n";
        let p = parse_subtitles(vtt).unwrap();
        assert_eq!(p.cues.len(), 1);
        assert_eq!((p.cues[0].start_ms, p.cues[0].end_ms), (62_000, 63_500));
        assert_eq!(p.cues[0].text, "Hej med dig");
        assert_eq!(p.skipped_blocks, 0);
    }

    #[test]
    fn voice_with_class_and_multiple_voices_in_one_cue() {
        let vtt = "WEBVTT\n\n00:00:00.000 --> 00:00:03.000\n<v.loud Anna>Ja.</v><v Bo>Nej.</v>\n";
        let p = parse_subtitles(vtt).unwrap();
        assert_eq!(p.cues.len(), 2);
        assert_eq!(p.cues[0].speaker.as_deref(), Some("Anna"));
        assert_eq!(p.cues[1].speaker.as_deref(), Some("Bo"));
        assert_eq!(p.cues[1].start_ms, 0);
    }

    #[test]
    fn zoom_inline_speaker_convention_needs_a_majority() {
        let zoom = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:02.000\nAnna Test: Hej alle\n\n2\n00:00:02.000 --> 00:00:03.000\nfortsat samme tur\n\n3\n00:00:03.000 --> 00:00:04.000\nBo Eksempel: Hej Anna\n";
        let p = parse_subtitles(zoom).unwrap();
        assert_eq!(p.cues[0].speaker.as_deref(), Some("Anna Test"));
        assert_eq!(p.cues[0].text, "Hej alle");
        assert_eq!(p.cues[1].speaker.as_deref(), Some("Anna Test"));
        assert_eq!(p.cues[2].speaker.as_deref(), Some("Bo Eksempel"));

        // One colon in ordinary speech is not a speaker.
        let plain = "1\n00:00:01,000 --> 00:00:02,000\nNote: dette er tale\n\n2\n00:00:02,000 --> 00:00:03,000\nHelt almindelig sætning\n\n3\n00:00:03,000 --> 00:00:04,000\nOg endnu en\n";
        let p = parse_subtitles(plain).unwrap();
        assert!(p.cues.iter().all(|c| c.speaker.is_none()));
        assert_eq!(p.cues[0].text, "Note: dette er tale");
    }

    #[test]
    fn invalid_files_are_rejected_or_skipped() {
        assert_eq!(parse_subtitles(""), Err(SubtitleError::Empty));
        assert_eq!(
            parse_subtitles("\u{feff}  \n\r\n"),
            Err(SubtitleError::Empty)
        );
        assert_eq!(
            parse_subtitles("Bare almindelig tekst\nuden tider"),
            Err(SubtitleError::NoCues)
        );
        assert_eq!(parse_subtitles("WEBVTT\n\n"), Err(SubtitleError::NoCues));
        // End before start, bad clock values and garbage are skipped and counted.
        let mixed = "1\n00:00:05,000 --> 00:00:01,000\nbagvendt\n\n2\n00:61:00,000 --> 00:62:00,000\nugyldigt minut\n\n3\nikke tid --> nogetandet\ntekst\n\n4\n00:00:01,000 --> 00:00:02,000\nGyldig\n";
        let p = parse_subtitles(mixed).unwrap();
        assert_eq!(p.cues.len(), 1);
        assert_eq!(p.cues[0].text, "Gyldig");
        assert_eq!(p.skipped_blocks, 3);
        // Only invalid cues: no usable result.
        assert_eq!(
            parse_subtitles("1\n00:00:05,000 --> 00:00:01,000\nx\n"),
            Err(SubtitleError::NoCues)
        );
    }

    #[test]
    fn empty_cue_text_and_lone_angle_bracket() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\n\n\n2\n00:00:02,000 --> 00:00:03,000\n3 < 5 og <i>fed</i>\n";
        let p = parse_subtitles(srt).unwrap();
        assert_eq!(p.cues.len(), 1);
        assert_eq!(p.cues[0].text, "3 < 5 og fed");
    }

    #[test]
    fn prompt_text_groups_consecutive_speakers() {
        let p = parse_subtitles(
            "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\n<v A>Et</v>\n\n00:00:01.000 --> 00:00:02.000\n<v A>to</v>\n\n00:00:02.000 --> 00:00:03.000\n<v B>tre</v>\n",
        )
        .unwrap();
        assert_eq!(cues_to_prompt_text(&p.cues), "A: Et to\nB: tre");
    }
}
