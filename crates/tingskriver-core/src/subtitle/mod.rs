//! Import of existing transcripts (`.vtt`, `.srt`) as meetings without audio.

pub mod import;
pub mod parser;

pub use parser::{
    cues_to_prompt_text, parse_subtitles, Cue, ParsedSubtitles, SubtitleError, SubtitleFormat,
};
