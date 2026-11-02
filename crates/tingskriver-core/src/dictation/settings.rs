//! Dictation settings (stored as JSON in the dictation database).

use serde::{Deserialize, Serialize};

use super::logic::app_style::{self, AppStyleRule};
use super::logic::cleanup::Style;
use super::logic::hotkey::Chord;
use super::logic::residency::{self, Policy};
use super::modes::{Context, Mode, ModeCollection};

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
    pub modes: ModeCollection,
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
            self.engine = if provider == "syv" { "auto".into() } else { provider.clone() };
        }
        if let Some(model) = &mode.model {
            self.syv_model = model.clone();
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

    pub fn residency_policy(&self) -> Policy {
        Policy::from_minutes(self.model_idle_minutes, self.keep_warm_models.clone())
    }

    /// Validate and normalise user input before saving.
    pub fn normalized(mut self) -> Result<Self, String> {
        self.modes.validate()?;
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
        self.paste_timeout_ms = self.paste_timeout_ms.clamp(200, 5000);
        for s in &mut self.app_styles {
            s.process = s.process.trim().to_string();
            s.style = Style::parse(&s.style).as_str().to_string();
        }
        self.app_styles.retain(|s| !s.process.is_empty());
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
mod mode_tests {
    use super::*;

    #[test]
    fn mode_overrides_leave_global_settings_untouched() {
        let mut settings = DictationSettings::default();
        let mode = &mut settings.modes.modes[1];
        mode.provider = Some("local".into());
        mode.model = Some("chosen-model".into());
        mode.language = Some("en".into());
        mode.polish = Some(true);
        let (effective, mode) = settings.for_context(&Context { process: Some("Slack.exe"), ..Context::default() });
        assert_eq!(effective.engine, "local");
        assert_eq!(effective.syv_model, "chosen-model");
        assert!(effective.polish_enabled);
        assert_eq!(mode.unwrap().language.as_deref(), Some("en"));
        assert_eq!(settings.engine, "auto");
        assert!(!settings.polish_enabled);
    }

    #[test]
    fn old_settings_gain_presets_without_losing_preferences() {
        let settings: DictationSettings = serde_json::from_str(r#"{"engine":"local","polishEnabled":true}"#).unwrap();
        assert_eq!(settings.modes.modes.len(), 4);
        assert_eq!(settings.engine, "local");
        assert!(settings.polish_enabled);
    }
}
