//! Dictation settings (stored as JSON in the dictation database).

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::capture::CaptureOptions;
use super::logic::app_style::{self, AppStyleRule};
use super::logic::audio::Channel;
use super::logic::cleanup::Style;
use super::logic::hotkey::Chord;
use super::logic::modes;
use super::logic::residency::{self, Policy};
use super::logic::spoken::SpokenTable;
use super::modes::{Context, Mode, ModeCollection};

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
    /// "hold", "toggle", "hybrid" or "double-tap" (the existing default).
    pub shortcut_mode: super::logic::gesture::ShortcutMode,
    pub hybrid_threshold_ms: u64,
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
    /// Debug: wait after the target read the text before the original
    /// clipboard is restored.
    pub restore_clipboard_ms: u64,
    /// Debug: how long the target gets to read the text after Ctrl+V before
    /// the field counts as blocking paste and the text is typed.
    pub paste_timeout_ms: u64,
    pub app_styles: Vec<AppStyleSetting>,
    /// Unload a local speech model after this many idle minutes (0 = never).
    pub model_idle_minutes: u64,
    /// Local models that stay loaded however long they are idle.
    pub keep_warm_models: Vec<String>,
    pub language: String,
    pub format_numbers: bool,
    pub format_addresses: bool,
    /// Three or more equal words in a row become one ("nej nej nej" → "nej").
    pub collapse_stutters: bool,
    /// Input device for dictation (empty = the Windows default). Meetings keep their own.
    pub microphone: String,
    /// "mix", "left" or "right".
    pub channel: String,
    /// Keep the microphone open between dictations (faster start; Windows
    /// shows the microphone indicator all the time).
    pub keep_mic_open: bool,
    /// Play a sound when dictation is ready and when it stops.
    pub sound_enabled: bool,
    /// Volume of the sounds, 0-100.
    pub sound_volume: u32,
    /// Run a script from the hook folder on the finished text (off by default).
    pub text_hook_enabled: bool,
    /// File name (`.cmd` or `.ps1`) inside the hook folder.
    pub text_hook_script: String,
    pub modes: ModeCollection,
}

impl Default for DictationSettings {
    fn default() -> Self {
        DictationSettings {
            enabled: true,
            shortcut: Chord::DEFAULT.to_string(),
            shortcut_mode: super::logic::gesture::ShortcutMode::default(),
            hybrid_threshold_ms: 300,
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
            paste_timeout_ms: 1000,
            app_styles: app_style::default_rules()
                .into_iter()
                .map(|rule| AppStyleSetting {
                    process: rule.process,
                    style: rule.style.as_str().to_string(),
                })
                .collect(),
            model_idle_minutes: residency::DEFAULT_IDLE_MINUTES,
            keep_warm_models: Vec::new(),
            language: DEFAULT_LANGUAGE.into(),
            format_numbers: true,
            format_addresses: true,
            collapse_stutters: true,
            microphone: String::new(),
            channel: Channel::Mix.as_str().to_string(),
            keep_mic_open: false,
            sound_enabled: false,
            sound_volume: 40,
            text_hook_enabled: false,
            text_hook_script: String::new(),
            modes: ModeCollection::default(),
        }
    }
}

impl DictationSettings {
    pub fn for_context(&self, context: &Context<'_>) -> (Self, Option<Mode>) {
        let mode = self.modes.select(context).cloned();
        let mut settings = self.clone();
        if let Some(mode) = &mode {
            settings.apply_mode(mode);
        }
        (settings, mode)
    }

    pub fn apply_mode(&mut self, mode: &Mode) {
        if let Some(provider) = &mode.provider {
            if self.engine != "local" {
                self.engine = if provider == "syv" {
                    "auto".into()
                } else {
                    provider.clone()
                };
            }
        }
        if let Some(model) = &mode.model {
            self.syv_model = model.clone();
        }
        if let Some(language) = &mode.language {
            self.language = language.clone();
        }
        if let Some(polish) = mode.polish {
            self.polish_enabled = polish;
        }
    }

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

    pub fn capture_options(&self) -> CaptureOptions {
        CaptureOptions {
            microphone: self.microphone.trim().to_string(),
            channel: Channel::parse(&self.channel),
            keep_open: self.keep_mic_open,
        }
    }

    pub fn residency_policy(&self) -> Policy {
        Policy::from_minutes(self.model_idle_minutes, self.keep_warm_models.clone())
    }

    /// Validate and normalise user input before saving.
    pub fn normalized(mut self) -> Result<Self, String> {
        self.hybrid_threshold_ms = self.hybrid_threshold_ms.clamp(100, 1000);
        self.language = self.language.trim().to_lowercase();
        if self.language.is_empty() {
            self.language = DEFAULT_LANGUAGE.into();
        }
        if !modes::valid_language(&self.language) {
            return Err(format!("Ukendt sprogkode: {}", self.language));
        }
        self.modes.validate()?;
        self.shortcut = self.chord()?.label();
        if self.engine != "local" {
            self.engine = "auto".to_string();
        }
        if self.syv_endpoint.trim().is_empty() {
            self.syv_endpoint = SYV_DEFAULT_ENDPOINT.to_string();
        }
        self.syv_endpoint = self.syv_endpoint.trim().trim_end_matches('/').to_string();
        if !self.syv_endpoint.starts_with("https://")
            && !self.syv_endpoint.starts_with("http://localhost")
        {
            return Err("Syv.ai-adressen skal starte med https://".to_string());
        }
        if self.syv_model.trim().is_empty() {
            self.syv_model = SYV_DEFAULT_MODEL.to_string();
        }
        self.fallback_after_ms = self.fallback_after_ms.clamp(500, 10_000);
        self.polish_budget_ms = self.polish_budget_ms.clamp(500, 10_000);
        self.restore_clipboard_ms = self.restore_clipboard_ms.clamp(100, 2000);
        self.paste_timeout_ms = self.paste_timeout_ms.clamp(200, 5000);
        for s in &mut self.app_styles {
            s.process = s.process.trim().to_string();
            s.style = Style::parse(&s.style).as_str().to_string();
        }
        self.app_styles.retain(|s| !s.process.is_empty());
        self.microphone = self.microphone.trim().to_string();
        self.channel = Channel::parse(&self.channel).as_str().to_string();
        self.sound_volume = self.sound_volume.min(100);
        self.text_hook_script = self.text_hook_script.trim().to_string();
        // Switching the hook on before a script is chosen is allowed (the
        // hook then does nothing); a script name that is set must be valid.
        if self.text_hook_enabled && !self.text_hook_script.is_empty() {
            super::text_hook::script_path(Path::new(""), &self.text_hook_script)?;
        } else if super::text_hook::script_path(Path::new(""), &self.text_hook_script).is_err() {
            self.text_hook_script.clear();
        }
        self.model_idle_minutes = self.model_idle_minutes.min(24 * 60);
        self.keep_warm_models = self
            .keep_warm_models
            .iter()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();
        self.keep_warm_models.dedup();
        Ok(self)
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod mode_tests {
    use super::*;

    #[test]
    fn hybrid_threshold_defaults_and_bounds() {
        let old: DictationSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.hybrid_threshold_ms, 300);
        for (input, expected) in [
            (0, 100),
            (100, 100),
            (300, 300),
            (650, 650),
            (1000, 1000),
            (u64::MAX, 1000),
        ] {
            let mut settings = old.clone();
            settings.hybrid_threshold_ms = input;
            let settings = settings.normalized().unwrap();
            assert_eq!(settings.hybrid_threshold_ms, expected);
            let saved = serde_json::to_string(&settings).unwrap();
            assert_eq!(
                serde_json::from_str::<DictationSettings>(&saved)
                    .unwrap()
                    .hybrid_threshold_ms,
                expected
            );
        }
    }

    #[test]
    fn mode_overrides_leave_global_settings_untouched() {
        let mut settings = DictationSettings::default();
        let mode = &mut settings.modes.modes[1];
        mode.provider = Some("local".into());
        mode.model = Some("chosen-model".into());
        mode.language = Some("en".into());
        mode.polish = Some(true);
        let (effective, mode) = settings.for_context(&Context {
            process: Some("Slack.exe"),
            ..Context::default()
        });
        assert_eq!(effective.engine, "local");
        assert_eq!(effective.syv_model, "chosen-model");
        assert!(effective.polish_enabled);
        assert_eq!(mode.unwrap().language.as_deref(), Some("en"));
        assert_eq!(settings.engine, "auto");
        assert!(!settings.polish_enabled);
    }

    #[test]
    fn old_settings_gain_presets_without_losing_preferences() {
        let settings: DictationSettings =
            serde_json::from_str(r#"{"engine":"local","polishEnabled":true}"#).unwrap();
        assert_eq!(settings.modes.modes.len(), 4);
        assert_eq!(
            settings.shortcut_mode,
            super::super::logic::gesture::ShortcutMode::DoubleTap
        );
        assert_eq!(settings.engine, "local");
        assert!(settings.polish_enabled);
    }
}

/// The spoken signs table: built-in plus `dictation_spoken.json` in `dir`.
/// A broken file gives the built-in table and the error to show.
pub async fn load_spoken_table(dir: &Path) -> (SpokenTable, Option<String>) {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        #[allow(clippy::disallowed_methods)]
        fn blocking(dir: &Path) -> (SpokenTable, Option<String>) {
            let path = dir.join(SPOKEN_FILE);
            match std::fs::read_to_string(&path) {
                Ok(json) => match SpokenTable::with_user(&json) {
                    Ok(table) => (table, None),
                    Err(e) => (
                        SpokenTable::builtin(),
                        Some(format!("{e} ({})", path.display())),
                    ),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    (SpokenTable::builtin(), None)
                }
                Err(e) => (
                    SpokenTable::builtin(),
                    Some(format!("{} kan ikke læses: {e}", path.display())),
                ),
            }
        }
        blocking(&dir)
    })
    .await
    .unwrap_or_else(|e| {
        (
            SpokenTable::builtin(),
            Some(format!("Could not load spoken rules: {e}")),
        )
    })
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod spoken_tests {
    use super::*;
    #[test]
    fn spoken_file_is_optional_and_errors_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (table, error) =
            crate::audio::common::assert_uses_blocking_pool(load_spoken_table(dir.path()));
        assert_eq!((table, error), (SpokenTable::builtin(), None));

        std::fs::write(dir.path().join(SPOKEN_FILE), "{ ikke json").unwrap();
        let (table, error) =
            crate::audio::common::assert_uses_blocking_pool(load_spoken_table(dir.path()));
        assert_eq!(table, SpokenTable::builtin());
        assert!(error.is_some());

        let json = r#"{"version":1,"entries":[{"phrases":["pil"],"action":{"type":"symbol","text":"→","spaceBefore":true,"spaceAfter":true}}]}"#;
        std::fs::write(dir.path().join(SPOKEN_FILE), json).unwrap();
        let (table, error) =
            crate::audio::common::assert_uses_blocking_pool(load_spoken_table(dir.path()));
        assert!(error.is_none());
        assert!(table.action_for("pil").is_some());
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod text_settings_tests {
    use super::*;

    #[test]
    fn old_settings_keep_danish_formatting_defaults() {
        let settings: DictationSettings =
            serde_json::from_str(r#"{"engine":"local","polishEnabled":true}"#).unwrap();
        assert_eq!(settings.language, "da");
        assert!(settings.format_numbers && settings.format_addresses);
        assert!(settings.polish_enabled);
        assert_eq!(settings.modes.modes.len(), 4);
    }

    #[test]
    fn local_only_wins_over_mode_provider() {
        let mut settings = DictationSettings {
            engine: "local".into(),
            ..Default::default()
        };
        let mut mode = settings.modes.modes[0].clone();
        mode.provider = Some("syv".into());
        settings.apply_mode(&mode);
        assert_eq!(settings.engine, "local");
    }

    #[test]
    fn new_text_options_have_safe_defaults_and_validate_the_hook() {
        let old: DictationSettings = serde_json::from_str(r#"{"engine":"local"}"#).unwrap();
        assert!(old.collapse_stutters);
        assert!(!old.text_hook_enabled);
        assert_eq!((old.microphone.as_str(), old.channel.as_str()), ("", "mix"));
        assert!(!old.keep_mic_open && !old.sound_enabled);
        let loud = DictationSettings {
            sound_volume: 500,
            channel: "RIGHT".into(),
            microphone: " USB ".into(),
            ..Default::default()
        };
        let loud = loud.normalized().unwrap();
        assert_eq!(
            (
                loud.sound_volume,
                loud.channel.as_str(),
                loud.microphone.as_str()
            ),
            (100, "right", "USB")
        );
        let on = DictationSettings {
            text_hook_enabled: true,
            ..Default::default()
        };
        assert!(on.clone().normalized().unwrap().text_hook_enabled);
        let on = DictationSettings {
            text_hook_script: "../x.cmd".into(),
            ..on
        };
        assert!(on.normalized().is_err());
        let ok = DictationSettings {
            text_hook_enabled: true,
            text_hook_script: " ret.ps1 ".into(),
            ..Default::default()
        };
        assert_eq!(ok.normalized().unwrap().text_hook_script, "ret.ps1");
        let off = DictationSettings {
            text_hook_script: "a&b.cmd".into(),
            ..Default::default()
        };
        assert_eq!(off.normalized().unwrap().text_hook_script, "");
    }

    #[test]
    fn language_is_normalized_and_validated() {
        let mut settings = DictationSettings {
            language: " EN ".into(),
            ..Default::default()
        };
        assert_eq!(settings.clone().normalized().unwrap().language, "en");
        settings.language = "dansk".into();
        assert!(settings.normalized().is_err());
    }
}
