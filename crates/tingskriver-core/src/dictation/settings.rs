//! Dictation settings (stored as JSON in the dictation database).

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::logic::app_style::{self, AppStyleRule};
use super::logic::cleanup::Style;
use super::logic::hotkey::Chord;
use super::logic::modes::{self, Mode};
use super::logic::spoken::SpokenTable;

/// The user's changes to the spoken signs and commands, in the app data folder.
pub const SPOKEN_FILE: &str = "dictation_spoken.json";

/// Default speech language. Never "auto-translate".
pub const DEFAULT_LANGUAGE: &str = "da";

pub const SYV_DEFAULT_ENDPOINT: &str = "https://platform.syv.ai/v1";
pub const SYV_DEFAULT_MODEL: &str = "syv-transcribe";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStyleSetting {
    pub process: String,
    /// "normal" or "raw".
    pub style: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DictationSettings {
    pub enabled: bool,
    pub shortcut: String,
    /// "auto" (Syv.ai with key, otherwise local) or "local".
    pub engine: String,
    pub syv_endpoint: String,
    pub syv_model: String,
    /// Fall back to the local model when Syv.ai is slower than this.
    pub fallback_after_ms: u64,
    pub polish_enabled: bool,
    /// "builtin-ai" or "ollama".
    pub polish_provider: String,
    pub polish_model: String,
    pub polish_endpoint: String,
    pub polish_budget_ms: u64,
    pub history_enabled: bool,
    /// Keep the recording for 24 hours alongside the history entry.
    pub keep_audio: bool,
    pub restore_clipboard_ms: u64,
    pub app_styles: Vec<AppStyleSetting>,
    /// Speech language ("da"). Modes can override it. Text is never translated.
    pub language: String,
    /// Danish number, date, time and amount formatting in layer 1.
    pub format_numbers: bool,
    /// E-mail and web addresses from speech ("snabel-a").
    pub format_addresses: bool,
    /// Dictation modes; the first one matching the foreground window is used.
    pub modes: Vec<Mode>,
}

impl Default for DictationSettings {
    fn default() -> Self {
        DictationSettings {
            enabled: true,
            shortcut: Chord::DEFAULT.to_string(),
            engine: "auto".to_string(),
            syv_endpoint: SYV_DEFAULT_ENDPOINT.to_string(),
            syv_model: SYV_DEFAULT_MODEL.to_string(),
            fallback_after_ms: 2000,
            polish_enabled: false,
            polish_provider: "builtin-ai".to_string(),
            polish_model: String::new(),
            polish_endpoint: String::new(),
            polish_budget_ms: 2000,
            history_enabled: true,
            keep_audio: true,
            restore_clipboard_ms: 300,
            app_styles: app_style::default_rules()
                .into_iter()
                .map(|rule| AppStyleSetting {
                    process: rule.process,
                    style: rule.style.as_str().to_string(),
                })
                .collect(),
            language: DEFAULT_LANGUAGE.to_string(),
            format_numbers: true,
            format_addresses: true,
            modes: modes::builtin_modes(),
        }
    }
}

impl DictationSettings {
    pub fn chord(&self) -> Result<Chord, String> {
        Chord::parse(&self.shortcut)
    }

    pub fn style_rules(&self) -> Vec<AppStyleRule> {
        self.app_styles
            .iter()
            .filter(|s| !s.process.trim().is_empty())
            .map(|s| AppStyleRule {
                process: s.process.trim().to_string(),
                style: Style::parse(&s.style),
            })
            .collect()
    }

    /// Validate and normalise user input before saving.
    pub fn normalized(mut self) -> Result<Self, String> {
        self.shortcut = self.chord()?.label();
        if self.engine != "local" {
            self.engine = "auto".to_string();
        }
        if self.syv_endpoint.trim().is_empty() {
            self.syv_endpoint = SYV_DEFAULT_ENDPOINT.to_string();
        }
        self.syv_endpoint = self.syv_endpoint.trim().trim_end_matches('/').to_string();
        if !self.syv_endpoint.starts_with("https://") && !self.syv_endpoint.starts_with("http://localhost") {
            return Err("Syv.ai-adressen skal starte med https://".to_string());
        }
        if self.syv_model.trim().is_empty() {
            self.syv_model = SYV_DEFAULT_MODEL.to_string();
        }
        self.fallback_after_ms = self.fallback_after_ms.clamp(500, 10_000);
        self.polish_budget_ms = self.polish_budget_ms.clamp(500, 10_000);
        self.restore_clipboard_ms = self.restore_clipboard_ms.clamp(100, 2000);
        for s in &mut self.app_styles {
            s.process = s.process.trim().to_string();
            s.style = Style::parse(&s.style).as_str().to_string();
        }
        self.app_styles.retain(|s| !s.process.is_empty());
        self.language = self.language.trim().to_lowercase();
        if self.language.is_empty() {
            self.language = DEFAULT_LANGUAGE.to_string();
        }
        if !modes::valid_language(&self.language) {
            return Err(format!(
                "Ukendt sprogkode \"{}\" (brug fx da eller en)",
                self.language
            ));
        }
        self.modes = if self.modes.is_empty() {
            modes::builtin_modes()
        } else {
            modes::validate(self.modes)?
        };
        Ok(self)
    }

    /// The settings a mode falls back to.
    pub fn mode_defaults(&self) -> modes::Defaults {
        modes::Defaults {
            engine: self.engine.clone(),
            model: self.syv_model.clone(),
            language: self.language.clone(),
            polish: self.polish_enabled,
            format_numbers: self.format_numbers,
            format_addresses: self.format_addresses,
        }
    }
}

/// The spoken signs table: built-in plus `dictation_spoken.json` in `dir`.
/// A broken file gives the built-in table and the error to show.
pub fn load_spoken_table(dir: &Path) -> (SpokenTable, Option<String>) {
    let path = dir.join(SPOKEN_FILE);
    match std::fs::read_to_string(&path) {
        Ok(json) => match SpokenTable::with_user(&json) {
            Ok(table) => (table, None),
            Err(e) => (
                SpokenTable::builtin(),
                Some(format!("{e} ({})", path.display())),
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (SpokenTable::builtin(), None),
        Err(e) => (
            SpokenTable::builtin(),
            Some(format!("{} kan ikke læses: {e}", path.display())),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_danish_without_translation() {
        let s = DictationSettings::default();
        assert_eq!(s.language, "da");
        assert!(s.format_numbers && s.format_addresses);
        assert_eq!(s.modes.len(), 4);
        // Polishing (the only step that could rewrite words) is off by default.
        assert!(!s.polish_enabled);
    }

    #[test]
    fn old_settings_json_gets_new_defaults() {
        let old = r#"{"enabled":true,"shortcut":"Ctrl+Win","engine":"auto","appStyles":[{"process":"vim.exe","style":"raw"}]}"#;
        let s: DictationSettings = serde_json::from_str(old).unwrap();
        assert_eq!(s.language, "da");
        assert_eq!(s.modes, modes::builtin_modes());
        assert_eq!(s.app_styles.len(), 1);
    }

    #[test]
    fn language_and_modes_are_validated() {
        let mut s = DictationSettings {
            language: " EN ".into(),
            ..DictationSettings::default()
        };
        assert_eq!(s.clone().normalized().unwrap().language, "en");
        s.language = "dansk".into();
        assert!(s.clone().normalized().is_err());
        s.language = String::new();
        s.modes.clear();
        let n = s.normalized().unwrap();
        assert_eq!((n.language.as_str(), n.modes.len()), ("da", 4));
    }

    #[test]
    fn spoken_file_is_optional_and_errors_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (table, error) = load_spoken_table(dir.path());
        assert_eq!((table, error), (SpokenTable::builtin(), None));

        std::fs::write(dir.path().join(SPOKEN_FILE), "{ ikke json").unwrap();
        let (table, error) = load_spoken_table(dir.path());
        assert_eq!(table, SpokenTable::builtin());
        assert!(error.is_some());

        let json = r#"{"version":1,"entries":[{"phrases":["pil"],"action":{"type":"symbol","text":"→","spaceBefore":true,"spaceAfter":true}}]}"#;
        std::fs::write(dir.path().join(SPOKEN_FILE), json).unwrap();
        let (table, error) = load_spoken_table(dir.path());
        assert!(error.is_none());
        assert!(table.action_for("pil").is_some());
    }
}
