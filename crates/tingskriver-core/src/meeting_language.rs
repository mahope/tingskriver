//! Languages of a meeting (issue #209): the main language, extra spoken
//! languages and the language of the report.
//!
//! * The main language is always a transcription language, and the default
//!   language of the report. Default `da`.
//! * Extra spoken languages (for example English) give the transcription
//!   context. They never change the language of an existing transcript.
//! * The report is written in the main language when `summary_in_primary` is
//!   on (default). When it is off, the most spoken of the selected languages
//!   is used if it can be detected, otherwise the main language.
//!
//! Stored as JSON in the app data folder. A missing file means the defaults,
//! which equal the behaviour from before the setting existed (Danish
//! transcription and Danish reports).

use std::path::Path;

use serde::{Deserialize, Serialize};

/// File in the app data folder.
pub const FILE: &str = "meeting_language.json";

pub const DEFAULT_PRIMARY: &str = "da";

/// Languages that can be chosen: ISO 639-1 code and Danish name (lower case,
/// as used in the middle of a sentence). Every code is accepted by the
/// transcription providers and has a report language name.
pub const CHOICES: &[(&str, &str)] = &[
    ("da", "dansk"),
    ("en", "engelsk"),
    ("sv", "svensk"),
    ("no", "norsk"),
    ("de", "tysk"),
    ("fr", "fransk"),
    ("es", "spansk"),
    ("nl", "hollandsk"),
    ("fi", "finsk"),
];

/// Capitalised Danish name for a button or label ("Dansk").
pub fn label(code: &str) -> String {
    let name = danish_name(code).unwrap_or(code);
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Danish name of a selectable language code.
pub fn danish_name(code: &str) -> Option<&'static str> {
    CHOICES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MeetingLanguages {
    /// Main language: chat, report and always a transcription language.
    pub primary: String,
    /// Other languages expected in the conversation.
    pub extra: Vec<String>,
    /// Always write the report in the main language.
    pub summary_in_primary: bool,
}

impl Default for MeetingLanguages {
    fn default() -> Self {
        Self {
            primary: DEFAULT_PRIMARY.into(),
            extra: Vec::new(),
            summary_in_primary: true,
        }
    }
}

impl MeetingLanguages {
    /// Trims, lower-cases and validates; extras lose duplicates and the main
    /// language. An empty main language becomes the default.
    pub fn normalized(mut self) -> Result<Self, String> {
        self.primary = self.primary.trim().to_lowercase();
        if self.primary.is_empty() {
            self.primary = DEFAULT_PRIMARY.into();
        }
        if danish_name(&self.primary).is_none() {
            return Err(format!("Ukendt sprogkode: {}", self.primary));
        }
        let mut extra: Vec<String> = Vec::new();
        for code in &self.extra {
            let code = code.trim().to_lowercase();
            if code.is_empty() {
                continue;
            }
            if danish_name(&code).is_none() {
                return Err(format!("Ukendt sprogkode: {code}"));
            }
            if code != self.primary && !extra.contains(&code) {
                extra.push(code);
            }
        }
        self.extra = extra;
        Ok(self)
    }

    /// Makes `code` the main language. If it was an extra language it stops
    /// being one. Existing transcripts are not touched.
    pub fn with_primary(mut self, code: &str) -> Result<Self, String> {
        self.primary = code.to_string();
        self.normalized()
    }

    /// Adds `code` to the extra languages, or removes it when present. The
    /// main language cannot be an extra language.
    pub fn with_extra_toggled(mut self, code: &str) -> Result<Self, String> {
        if let Some(at) = self.extra.iter().position(|c| c == code) {
            self.extra.remove(at);
        } else {
            self.extra.push(code.to_string());
        }
        self.normalized()
    }

    /// The main language first, then the extra languages.
    pub fn transcription_languages(&self) -> Vec<String> {
        std::iter::once(self.primary.clone())
            .chain(self.extra.iter().cloned())
            .collect()
    }

    /// The language of the report. `override_language` is the choice made on
    /// the meeting itself and always wins. `detected` is the most spoken
    /// language of the transcript; it only counts when it is one of the
    /// selected languages.
    pub fn summary_language(
        &self,
        override_language: Option<&str>,
        detected: Option<&str>,
    ) -> String {
        if let Some(code) = override_language.map(str::trim).filter(|c| !c.is_empty()) {
            return code.to_string();
        }
        if !self.summary_in_primary {
            let selected = self.transcription_languages();
            if let Some(code) = detected.map(|c| c.trim().to_lowercase()) {
                if selected.contains(&code) {
                    return code;
                }
            }
        }
        self.primary.clone()
    }

    /// One sentence for the transcription prompt that names the extra
    /// languages, or `None` when there are none (the request then stays
    /// exactly as before).
    pub fn prompt_hint(&self) -> Option<String> {
        extra_languages_hint(&self.primary, &self.extra)
    }

    /// Reads the settings. A missing file gives the defaults; an unreadable or
    /// invalid file is an error instead of silently changing the languages.
    /// Blocking: async code uses [`Self::load_async`].
    #[allow(clippy::disallowed_methods)]
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        let path = data_dir.join(FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("Kunne ikke læse sprogindstillingerne: {e}")),
        };
        serde_json::from_str::<Self>(&text)
            .map_err(|e| format!("Sprogindstillingerne er ugyldige: {e}"))?
            .normalized()
    }

    /// [`Self::load`] on a blocking worker.
    pub async fn load_async(data_dir: std::path::PathBuf) -> Result<Self, String> {
        tokio::task::spawn_blocking(move || Self::load(&data_dir))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Validates and writes the settings (temp file, then rename).
    #[allow(clippy::disallowed_methods)]
    pub fn save(self, data_dir: &Path) -> Result<Self, String> {
        let settings = self.normalized()?;
        let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        let path = data_dir.join(FILE);
        let temp = data_dir.join(format!("{FILE}.tmp"));
        std::fs::write(&temp, json).map_err(|e| format!("Kunne ikke gemme sproget: {e}"))?;
        std::fs::rename(&temp, &path).map_err(|e| format!("Kunne ikke gemme sproget: {e}"))?;
        Ok(settings)
    }
}

/// "Samtalen foregår på dansk og engelsk." for a main language with extras.
pub fn extra_languages_hint(primary: &str, extra: &[String]) -> Option<String> {
    let mut names: Vec<&str> = Vec::new();
    for code in std::iter::once(primary).chain(extra.iter().map(String::as_str)) {
        let name = danish_name(code)?;
        if !names.contains(&name) {
            names.push(name);
        }
    }
    if names.len() < 2 {
        return None;
    }
    let last = names.pop()?;
    Some(format!(
        "Samtalen foregår på {} og {last}.",
        names.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn languages(primary: &str, extra: &[&str], in_primary: bool) -> MeetingLanguages {
        MeetingLanguages {
            primary: primary.into(),
            extra: extra.iter().map(|c| c.to_string()).collect(),
            summary_in_primary: in_primary,
        }
    }

    #[test]
    fn defaults_are_danish_with_the_report_in_the_main_language() {
        let d = MeetingLanguages::default();
        assert_eq!(d.primary, "da");
        assert!(d.extra.is_empty());
        assert!(d.summary_in_primary);
        assert_eq!(d.transcription_languages(), ["da"]);
        assert_eq!(d.prompt_hint(), None);
    }

    #[test]
    fn missing_file_is_the_old_behaviour_and_old_json_is_read() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            MeetingLanguages::load(dir.path()).unwrap(),
            MeetingLanguages::default()
        );
        // A file with only some fields (for example from an older version)
        // keeps the defaults for the rest.
        std::fs::write(dir.path().join(FILE), r#"{"extra":["en"]}"#).unwrap();
        let loaded = MeetingLanguages::load(dir.path()).unwrap();
        assert_eq!(loaded.primary, "da");
        assert_eq!(loaded.extra, ["en"]);
        assert!(loaded.summary_in_primary);
        std::fs::write(dir.path().join(FILE), "{}").unwrap();
        assert_eq!(
            MeetingLanguages::load(dir.path()).unwrap(),
            MeetingLanguages::default()
        );
    }

    #[test]
    fn invalid_file_is_an_error_not_a_silent_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "not json").unwrap();
        assert!(MeetingLanguages::load(dir.path()).is_err());
        std::fs::write(dir.path().join(FILE), r#"{"primary":"xx"}"#).unwrap();
        assert!(MeetingLanguages::load(dir.path()).is_err());
    }

    #[test]
    fn save_round_trips_and_normalizes() {
        let dir = tempfile::tempdir().unwrap();
        let saved = languages(" DA ", &["EN", "da", "en", ""], true)
            .save(dir.path())
            .unwrap();
        assert_eq!(saved, languages("da", &["en"], true));
        assert_eq!(MeetingLanguages::load(dir.path()).unwrap(), saved);
        assert!(languages("da", &["klingon"], true)
            .save(dir.path())
            .is_err());
        // The failed save left the old file alone.
        assert_eq!(MeetingLanguages::load(dir.path()).unwrap(), saved);
    }

    #[test]
    fn main_language_is_always_a_transcription_language() {
        let l = languages("da", &["en", "sv"], true);
        assert_eq!(l.transcription_languages(), ["da", "en", "sv"]);
    }

    #[test]
    fn report_follows_main_language_by_default_even_when_english_dominates() {
        let l = languages("da", &["en"], true);
        assert_eq!(l.summary_language(None, Some("en")), "da");
        assert_eq!(l.summary_language(None, None), "da");
    }

    #[test]
    fn report_follows_the_most_spoken_selected_language_when_allowed() {
        let l = languages("da", &["en"], false);
        assert_eq!(l.summary_language(None, Some("en")), "en");
        assert_eq!(l.summary_language(None, Some("da")), "da");
        // Not selected, or not detectable: the main language.
        assert_eq!(l.summary_language(None, Some("de")), "da");
        assert_eq!(l.summary_language(None, None), "da");
    }

    #[test]
    fn editing_helpers_keep_the_rules() {
        let l = MeetingLanguages::default()
            .with_extra_toggled("en")
            .unwrap();
        assert_eq!(l.extra, ["en"]);
        // English becomes the main language: it leaves the extra list.
        let l = l.with_primary("en").unwrap();
        assert_eq!((l.primary.as_str(), l.extra.len()), ("en", 0));
        // The main language cannot be added as an extra language.
        assert!(l.clone().with_extra_toggled("en").unwrap().extra.is_empty());
        let l = l
            .with_extra_toggled("da")
            .unwrap()
            .with_extra_toggled("da")
            .unwrap();
        assert!(l.extra.is_empty());
        assert!(MeetingLanguages::default().with_primary("xx").is_err());
        assert_eq!(label("da"), "Dansk");
    }

    #[test]
    fn the_choice_on_the_meeting_wins() {
        let l = languages("da", &["en"], true);
        assert_eq!(l.summary_language(Some("en"), Some("da")), "en");
        assert_eq!(l.summary_language(Some("  "), Some("en")), "da");
    }

    #[test]
    fn hint_names_the_languages_in_danish() {
        assert_eq!(
            languages("da", &["en"], true).prompt_hint().as_deref(),
            Some("Samtalen foregår på dansk og engelsk.")
        );
        assert_eq!(
            languages("da", &["en", "sv"], true)
                .prompt_hint()
                .as_deref(),
            Some("Samtalen foregår på dansk, engelsk og svensk.")
        );
    }

    #[test]
    fn every_choice_has_a_report_language_name() {
        for (code, _) in CHOICES {
            assert!(
                crate::summary::processor::language_name_from_code(code).is_some(),
                "{code}"
            );
        }
    }
}
