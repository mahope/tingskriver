//! Versioned dictation modes and deterministic foreground matching.

use serde::{Deserialize, Serialize};

use super::logic::app_style;
use super::logic::cleanup::Style;

pub use super::logic::modes::CommaStyle;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModeStyle {
    #[default]
    Normal,
    Formal,
    Short,
    Raw,
}

impl ModeStyle {
    pub fn cleanup(self) -> Style {
        if self == Self::Raw {
            Style::Raw
        } else {
            Style::Normal
        }
    }

    pub fn instruction(self) -> &'static str {
        match self {
            Self::Normal => "Bevar tonen.",
            Self::Formal => "Skriv på formelt dansk.",
            Self::Short => "Skriv kort og uformelt.",
            Self::Raw => "Bevar teksten ordret.",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Output {
    #[default]
    Insert,
    InsertAndEnter {
        trigger: String,
    },
}

impl Output {
    /// Strip only a final whole phrase, before cleanup or AI can change it.
    pub fn prepare(&self, text: &str, code_editor: bool) -> (String, bool) {
        if let Self::InsertAndEnter { trigger } = self {
            let end = text.trim_end_matches(|c: char| c.is_whitespace() || ".!?".contains(c));
            let trigger = trigger.trim();
            if !code_editor && !trigger.is_empty() {
                let mut remaining = end;
                for word in trigger.split_whitespace().rev() {
                    let start = remaining
                        .rfind(char::is_whitespace)
                        .map(|i| i + remaining[i..].chars().next().unwrap().len_utf8())
                        .unwrap_or(0);
                    if remaining[start..].to_lowercase() != word.to_lowercase() {
                        return (text.to_string(), false);
                    }
                    remaining = remaining[..start].trim_end();
                }
                if !remaining.is_empty() {
                    return (remaining.to_string(), true);
                }
            }
        }
        (text.to_string(), false)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Rule {
    Program(String),
    WindowClass(String),
    Website(String),
    TriggerWord(String),
    Shortcut(String),
}

#[derive(Default)]
pub struct Context<'a> {
    pub process: Option<&'a str>,
    pub window_class: Option<&'a str>,
    pub title: Option<&'a str>,
    pub trigger_word: Option<&'a str>,
    pub shortcut: Option<&'a str>,
}

impl Rule {
    fn matches(&self, context: &Context<'_>) -> bool {
        let eq = |value: Option<&str>, expected: &str| {
            value.is_some_and(|v| v.eq_ignore_ascii_case(expected))
        };
        match self {
            Self::Program(p) => context
                .process
                .is_some_and(|v| app_style::same_program(v, p)),
            Self::WindowClass(c) => eq(context.window_class, c),
            Self::Website(s) => {
                !s.is_empty()
                    && context
                        .title
                        .is_some_and(|v| v.to_lowercase().contains(&s.to_lowercase()))
            }
            Self::TriggerWord(w) => eq(context.trigger_word, w),
            Self::Shortcut(s) => context.shortcut.is_some_and(|v| {
                let parse = super::logic::hotkey::Chord::parse;
                matches!((parse(v), parse(s)), (Ok(a), Ok(b)) if a == b)
            }),
        }
    }

    fn value(&self) -> &str {
        match self {
            Self::Program(v)
            | Self::WindowClass(v)
            | Self::Website(v)
            | Self::TriggerWord(v)
            | Self::Shortcut(v) => v,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Formatting {
    /// Optional instructions for number formatting, e.g. "dansk" or "ordret".
    pub number_format: Option<String>,
    /// Preserve foreign words rather than translating them.
    pub code_switching: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mode {
    pub id: String,
    pub name: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub language: Option<String>,
    pub style: ModeStyle,
    pub polish: Option<bool>,
    #[serde(default)]
    pub comma_style: CommaStyle,
    pub prompt: String,
    #[serde(default)]
    pub formatting: Formatting,
    pub output: Output,
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeCollection {
    pub version: u32,
    pub modes: Vec<Mode>,
}

impl Default for ModeCollection {
    fn default() -> Self {
        let mode = |id: &str, name: &str, style, rules| Mode {
            id: id.into(),
            name: name.into(),
            provider: None,
            model: None,
            language: Some("da".into()),
            style,
            polish: None,
            comma_style: CommaStyle::default(),
            prompt: String::new(),
            formatting: Formatting::default(),
            output: Output::Insert,
            rules,
        };
        Self {
            version: 1,
            modes: vec![
                mode("standard", "Standard", ModeStyle::Normal, vec![]),
                mode(
                    "chat",
                    "Chat",
                    ModeStyle::Short,
                    vec![
                        Rule::Program("Slack.exe".into()),
                        Rule::Program("ms-teams.exe".into()),
                    ],
                ),
                Mode {
                    polish: Some(false),
                    ..mode(
                        "code",
                        "Kode/rå",
                        ModeStyle::Raw,
                        app_style::default_rules()
                            .into_iter()
                            .map(|r| Rule::Program(r.process))
                            .collect(),
                    )
                },
                mode(
                    "formal",
                    "Formel",
                    ModeStyle::Formal,
                    vec![Rule::Program("Outlook.exe".into())],
                ),
            ],
        }
    }
}

impl ModeCollection {
    pub fn spoken<'a>(&self, text: &'a str, process: Option<&str>) -> Option<(&Mode, &'a str)> {
        if app_style::is_code_editor(process) {
            return None;
        }
        let text = text.trim_start();
        for mode in &self.modes {
            for rule in &mode.rules {
                if let Rule::TriggerWord(word) = rule {
                    let word = word.trim();
                    if !word.is_empty()
                        && text
                            .get(..word.len())
                            .is_some_and(|s| s.to_lowercase() == word.to_lowercase())
                    {
                        let rest = text.get(word.len()..)?;
                        if rest.starts_with(char::is_whitespace) && !rest.trim().is_empty() {
                            return Some((mode, rest.trim_start()));
                        }
                    }
                }
            }
        }
        None
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.modes.is_empty() {
            return Err("Ukendt tilstandsversion eller ingen tilstande".into());
        }
        let mut ids = std::collections::HashSet::new();
        for mode in &self.modes {
            for rule in &mode.rules {
                if let Rule::Shortcut(s) = rule {
                    super::logic::hotkey::Chord::parse(s)?;
                }
            }
            if mode.id.trim().is_empty() || mode.name.trim().is_empty() || !ids.insert(&mode.id) {
                return Err("Tilstande skal have et navn og et unikt id".into());
            }
            if mode
                .provider
                .as_deref()
                .is_some_and(|p| !["auto", "local", "syv"].contains(&p))
            {
                return Err("Ukendt dikteringsudbyder".into());
            }
            if mode
                .language
                .as_deref()
                .is_some_and(|s| s.trim().is_empty())
                || mode.rules.iter().any(|r| r.value().trim().is_empty())
                || matches!(&mode.output, Output::InsertAndEnter { trigger } if trigger.trim().is_empty())
            {
                return Err("Sprog, regler og udløserord må ikke være tomme".into());
            }
        }
        Ok(())
    }

    pub fn export(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn import(json: &str) -> Result<Self, String> {
        let modes: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        modes.validate()?;
        Ok(modes)
    }

    /// Editors take precedence, then explicit triggers, then foreground rules.
    pub fn select(&self, context: &Context<'_>) -> Option<&Mode> {
        if app_style::is_code_editor(context.process) {
            return self.modes.iter().find(|m| m.style == ModeStyle::Raw);
        }
        self.modes
            .iter()
            .find(|m| {
                m.rules.iter().any(|r| {
                    matches!(r, Rule::TriggerWord(_) | Rule::Shortcut(_)) && r.matches(context)
                })
            })
            .or_else(|| {
                self.modes
                    .iter()
                    .find(|m| m.rules.iter().any(|r| r.matches(context)))
            })
            .or_else(|| self.modes.first())
    }
}

impl Mode {
    pub fn local_model(&self, engine: super::logic::engine::Engine) -> Option<&str> {
        (engine == super::logic::engine::Engine::Local)
            .then_some(self.model.as_deref())
            .flatten()
    }

    pub fn transcription_changed(&self, previous: Option<&Self>) -> bool {
        previous.map(|m| (&m.provider, &m.model, &m.language))
            != Some((&self.provider, &self.model, &self.language))
    }

    pub fn prompt(&self) -> String {
        let mut prompt = format!("{} {}", self.style.instruction(), self.prompt);
        if let Some(format) = &self.formatting.number_format {
            prompt.push_str(&format!(" Talformat: {format}."));
        }
        if let Some(switching) = self.formatting.code_switching {
            prompt.push_str(if switching {
                " Bevar ord på andre sprog."
            } else {
                " Brug det valgte sprog konsekvent."
            });
        }
        prompt
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn comma_style_survives_mode_settings_round_trip() {
        let modes = super::ModeCollection::default();
        let mut json = serde_json::to_value(&modes).unwrap();
        json["modes"][0]["commaStyle"] = "grammatical".into();
        let restored = super::ModeCollection::import(&json.to_string()).unwrap();
        let saved = serde_json::to_value(&restored).unwrap();
        assert_eq!(saved["modes"][0]["commaStyle"], "grammatical");
    }

    #[test]
    fn legacy_modes_preserve_settings_and_default_to_grammatical_commas() {
        let modes = super::ModeCollection::default();
        assert!(modes
            .modes
            .iter()
            .all(|mode| mode.comma_style == super::CommaStyle::Grammatical));
        let mut json = serde_json::to_value(&modes).unwrap();
        for mode in json["modes"].as_array_mut().unwrap() {
            mode.as_object_mut().unwrap().remove("commaStyle");
        }
        let restored = super::ModeCollection::import(&json.to_string()).unwrap();
        assert_eq!(restored, modes);
        for style in [
            super::CommaStyle::Grammatical,
            super::CommaStyle::Clarity,
            super::CommaStyle::Unchanged,
        ] {
            let mut configured = restored.clone();
            configured.modes[0].comma_style = style;
            assert_eq!(
                super::ModeCollection::import(&configured.export().unwrap()).unwrap(),
                configured
            );
        }
    }

    #[test]
    fn explicit_provider_and_local_model_change_transcription() {
        let mut mode = super::ModeCollection::default().modes.remove(0);
        mode.provider = Some("auto".into());
        mode.model = Some("parakeet".into());
        let mut next = mode.clone();
        next.provider = Some("syv".into());
        assert!(next.transcription_changed(Some(&mode)));
        assert!(!mode.transcription_changed(Some(&mode)));
        let plan = super::super::logic::engine::plan(
            super::super::logic::engine::EnginePreference::Auto,
            false,
            true,
        )
        .unwrap();
        assert_eq!(mode.local_model(plan.primary), Some("parakeet"));
        assert_eq!(
            mode.local_model(super::super::logic::engine::Engine::Syv),
            None
        );
    }
    use super::*;

    #[test]
    fn spoken_prefix_selects_a_mode_without_eating_normal_words() {
        let mut modes = ModeCollection::default();
        modes.modes[1]
            .rules
            .push(Rule::TriggerWord("besked".into()));
        let (mode, text) = modes
            .spoken("Besked hej med dig", Some("Outlook.exe"))
            .unwrap();
        assert_eq!(mode.name, "Chat");
        assert_eq!(text, "hej med dig");
        assert!(modes.spoken("beskeden er klar", None).is_none());
        assert!(modes.spoken("en besked til dig", None).is_none());
        assert!(modes.spoken("besked hej", Some("Code.exe")).is_none());
    }

    #[test]
    #[ignore = "manual microbenchmark; run with --ignored --nocapture"]
    fn measure_foreground_matching() {
        use std::{hint::black_box, time::Instant};
        let modes = ModeCollection::default();
        let rules = app_style::default_rules();
        let process = "C:\\Office\\OUTLOOK.EXE";
        let old_base = |p: &str| {
            p.rsplit(['\\', '/'])
                .next()
                .unwrap()
                .trim()
                .to_lowercase()
                .trim_end_matches(".exe")
                .to_string()
        };
        let start = Instant::now();
        for _ in 0..100_000 {
            let name = old_base(black_box(process));
            black_box(rules.iter().find(|r| old_base(&r.process) == name));
        }
        let before = start.elapsed();
        let start = Instant::now();
        for _ in 0..100_000 {
            black_box(app_style::style_for(Some(black_box(process)), &rules));
        }
        let after = start.elapsed();
        let context = Context {
            process: Some(process),
            ..Context::default()
        };
        let start = Instant::now();
        for _ in 0..100_000 {
            black_box(modes.select(black_box(&context)));
        }
        println!(
            "100k lookups: previous app style {before:?}, current app style {after:?}, modes {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn outlook_selects_formal_with_a_fake_foreground() {
        let modes = ModeCollection::default();
        let foreground = Context {
            process: Some("C:\\Office\\OUTLOOK.EXE"),
            ..Context::default()
        };
        let mode = modes.select(&foreground).unwrap();
        assert_eq!(mode.name, "Formel");
        assert_eq!(mode.style, ModeStyle::Formal);
        assert_eq!(
            modes.select(&Context::default()).unwrap().style,
            ModeStyle::Normal
        );
        assert_eq!(
            modes
                .select(&Context {
                    process: Some("Slack.exe"),
                    ..Context::default()
                })
                .unwrap()
                .name,
            "Chat"
        );
        assert_eq!(
            modes
                .select(&Context {
                    process: Some("Code.exe"),
                    ..Context::default()
                })
                .unwrap()
                .name,
            "Kode/rå"
        );
    }

    #[test]
    fn enter_command_only_matches_a_final_whole_phrase() {
        let output = Output::InsertAndEnter {
            trigger: "send nu".into(),
        };
        for text in ["Hej send  nu", "Hej send\t nu!", "Hej send\u{a0}nu"] {
            assert_eq!(output.prepare(text, false), ("Hej".into(), true));
        }
        let spaced = Output::InsertAndEnter {
            trigger: "send  nu".into(),
        };
        assert_eq!(spaced.prepare("Hej send nu", false), ("Hej".into(), true));
        assert_eq!(
            output.prepare("Hej, SEND NU!", false),
            ("Hej,".into(), true)
        );
        for text in [
            "send nu og fortsæt",
            "gensend nu",
            "send nu",
            "Hej send nu i morgen",
        ] {
            assert_eq!(output.prepare(text, false), (text.into(), false));
        }
        assert_eq!(
            output.prepare("Hej send nu", true),
            ("Hej send nu".into(), false)
        );
        assert_eq!(
            Output::Insert.prepare("Hej send nu", false),
            ("Hej send nu".into(), false)
        );
    }

    #[test]
    fn versioned_json_round_trip_and_danish_presets() {
        let mut modes = ModeCollection::default();
        assert_eq!(
            ModeCollection::import(&modes.export().unwrap()).unwrap(),
            modes
        );
        assert_eq!(
            modes
                .modes
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["Standard", "Chat", "Kode/rå", "Formel"]
        );
        assert!(modes
            .modes
            .iter()
            .all(|m| m.language.as_deref() == Some("da")));
        let mode = &mut modes.modes[1];
        mode.provider = Some("syv".into());
        mode.model = Some("syv-transcribe".into());
        mode.language = Some("en".into());
        mode.polish = Some(true);
        mode.prompt = "Bevar æøå og fagord".into();
        mode.formatting = Formatting {
            number_format: Some("dansk".into()),
            code_switching: Some(true),
        };
        mode.output = Output::InsertAndEnter {
            trigger: "send nu".into(),
        };
        assert_eq!(
            ModeCollection::import(&modes.export().unwrap()).unwrap(),
            modes
        );
        assert!(ModeCollection::import(r#"{"version":99,"modes":[]}"#).is_err());
        assert!(ModeCollection::import(r#"{"version":1,"modes":[]}"#).is_err());
    }

    #[test]
    fn explicit_triggers_override_apps_but_editors_stay_raw() {
        let mut modes = ModeCollection::default();
        modes.modes[1].rules.extend([
            Rule::WindowClass("ChatWindow".into()),
            Rule::Website("slack.com".into()),
            Rule::TriggerWord("besked".into()),
            Rule::Shortcut("Ctrl+Alt+C".into()),
        ]);
        for context in [
            Context {
                window_class: Some("chatwindow"),
                ..Context::default()
            },
            Context {
                title: Some("Slack | slack.com"),
                ..Context::default()
            },
            Context {
                trigger_word: Some("besked"),
                ..Context::default()
            },
            Context {
                shortcut: Some("ctrl+alt+c"),
                ..Context::default()
            },
            Context {
                process: Some("Outlook.exe"),
                shortcut: Some("Ctrl+Alt+C"),
                ..Context::default()
            },
        ] {
            assert_eq!(modes.select(&context).unwrap().name, "Chat");
        }
        assert_eq!(
            modes
                .select(&Context {
                    process: Some("Code.exe"),
                    shortcut: Some("Ctrl+Alt+C"),
                    ..Context::default()
                })
                .unwrap()
                .style,
            ModeStyle::Raw
        );
    }

    #[test]
    fn rejects_ambiguous_or_unsafe_imports() {
        let mut modes = ModeCollection::default();
        modes.modes[1].id = modes.modes[0].id.clone();
        assert!(modes.validate().is_err());
        let mut modes = ModeCollection::default();
        modes.modes[1].output = Output::InsertAndEnter {
            trigger: "  ".into(),
        };
        assert!(modes.validate().is_err());
    }
}
