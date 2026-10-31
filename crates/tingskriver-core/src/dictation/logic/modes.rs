//! Dictation modes: provider, language, style, polishing, prompt and output
//! bundled under a name, chosen per program, window title, trigger word or
//! shortcut. Same program, same behaviour every time.
//!
//! Selection order: shortcut, trigger word at the start of the dictation,
//! program (exe), window class, window title, then the first mode (Standard).

use serde::{Deserialize, Serialize};

use super::app_style;
use super::cleanup::Style;

/// How the text leaves Tingskriver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Output {
    /// Insert the text.
    #[default]
    Paste,
    /// Insert the text and press Enter when the dictation ends with one of
    /// the mode's `enterTriggers` ("... send"). The trigger word is removed.
    PasteAndEnter,
}

/// When a mode is used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub enum ModeRule {
    /// Executable, e.g. "OUTLOOK.EXE" (with or without ".exe", any case).
    Program(String),
    /// Window class of the foreground window, e.g. "rctrl_renwnd32".
    WindowClass(String),
    /// Text in the window title, e.g. "Gmail" for a browser tab.
    TitleContains(String),
    /// Word said first in the dictation ("formel ..."); it is removed.
    TriggerWord(String),
    /// A shortcut of its own, e.g. "Ctrl+Win+F".
    Shortcut(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Mode {
    /// Stable id ("standard", "formel", "chat", "kode").
    pub id: String,
    /// Name shown in the UI and the pill.
    pub name: String,
    /// Transcription engine: "" (dictation setting), "auto" or "local".
    pub provider: String,
    /// Transcription model; "" uses the dictation setting.
    pub model: String,
    /// Speech language ("da", "en", ...); `None` uses the dictation setting.
    pub language: Option<String>,
    pub style: Style,
    /// LLM polishing; `None` uses the dictation setting.
    pub polish: Option<bool>,
    /// Extra instruction for polishing ("Skriv formelt ...").
    pub prompt: String,
    pub output: Output,
    pub enter_triggers: Vec<String>,
    /// Number, date and amount formatting; `None` uses the dictation setting.
    pub format_numbers: Option<bool>,
    /// E-mail and web addresses; `None` uses the dictation setting.
    pub format_addresses: Option<bool>,
    pub rules: Vec<ModeRule>,
}

impl Default for Mode {
    fn default() -> Self {
        Mode {
            id: String::new(),
            name: String::new(),
            provider: String::new(),
            model: String::new(),
            language: None,
            style: Style::Normal,
            polish: None,
            prompt: String::new(),
            output: Output::Paste,
            enter_triggers: Vec::new(),
            format_numbers: None,
            format_addresses: None,
            rules: Vec::new(),
        }
    }
}

fn programs(names: &[&str]) -> Vec<ModeRule> {
    names
        .iter()
        .map(|n| ModeRule::Program(n.to_string()))
        .collect()
}

fn titles(texts: &[&str]) -> Vec<ModeRule> {
    texts
        .iter()
        .map(|t| ModeRule::TitleContains(t.to_string()))
        .collect()
}

/// The modes Tingskriver ships with.
pub fn builtin_modes() -> Vec<Mode> {
    let mut formel_rules = programs(&[
        "OUTLOOK.EXE",
        "olk.exe",
        "Spark.exe",
        "Spark Desktop.exe",
        "thunderbird.exe",
        "WINWORD.EXE",
    ]);
    formel_rules.extend(titles(&["Outlook", "Gmail"]));

    let mut chat_rules = programs(&[
        "slack.exe",
        "Discord.exe",
        "ms-teams.exe",
        "Teams.exe",
        "WhatsApp.exe",
        "Signal.exe",
        "Messenger.exe",
        "Telegram.exe",
    ]);
    chat_rules.extend(titles(&["Slack", "Discord", "WhatsApp", "Messenger"]));

    let mut code_rules: Vec<ModeRule> = app_style::default_rules()
        .into_iter()
        .map(|r| ModeRule::Program(r.process))
        .collect();
    code_rules.extend(programs(&[
        "zed.exe",
        "sublime_text.exe",
        "notepad++.exe",
        "WindowsTerminal.exe",
        "mintty.exe",
    ]));
    let mut seen = Vec::new();
    code_rules.retain(|r| match r {
        ModeRule::Program(p) => {
            let key = p.to_lowercase();
            let new = !seen.contains(&key);
            seen.push(key);
            new
        }
        _ => true,
    });
    code_rules.extend(titles(&["GitHub Codespaces", "vscode.dev"]));

    vec![
        Mode {
            id: "standard".into(),
            name: "Standard".into(),
            ..Mode::default()
        },
        Mode {
            id: "formel".into(),
            name: "Formel".into(),
            prompt: "Skriv i en formel, høflig tone som i en arbejdsmail. Brug hele sætninger."
                .into(),
            rules: formel_rules,
            ..Mode::default()
        },
        Mode {
            id: "chat".into(),
            name: "Chat".into(),
            prompt: "Skriv kort og uformelt som en chatbesked. Ingen hilsen og ingen afslutning."
                .into(),
            output: Output::PasteAndEnter,
            enter_triggers: vec!["send".into(), "send den".into(), "send besked".into()],
            rules: chat_rules,
            ..Mode::default()
        },
        Mode {
            id: "kode".into(),
            name: "Kode/rå".into(),
            style: Style::Raw,
            polish: Some(false),
            format_numbers: Some(false),
            format_addresses: Some(false),
            rules: code_rules,
            ..Mode::default()
        },
    ]
}

/// The window the text goes to. Filled by the Windows layer; tests use fakes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForegroundWindow {
    pub process: Option<String>,
    pub class: Option<String>,
    pub title: Option<String>,
}

/// Why a mode was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchedBy {
    Shortcut,
    TriggerWord,
    Program,
    WindowClass,
    Title,
    /// The user's old "style per program" list said raw (code mode).
    LegacyStyle,
    Default,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeChoice {
    pub index: usize,
    pub matched_by: MatchedBy,
    /// Plain words of the trigger word to remove from the start of the text.
    pub trigger_words: usize,
}

fn same_program(a: &str, b: &str) -> bool {
    let base = |p: &str| {
        let file = p
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(p)
            .trim()
            .to_lowercase();
        file.strip_suffix(".exe")
            .map(str::to_string)
            .unwrap_or(file)
    };
    base(a) == base(b)
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Choose the mode for a dictation.
///
/// `shortcut` is the label of the mode shortcut that started the dictation,
/// if any. `transcript` is the raw text, used for trigger words.
/// `legacy_rules` is the old style-per-program list, still edited in the
/// settings. It decides for the programs it names (and the default editors
/// the user removed from it): "raw" picks the first raw mode, "normal" keeps
/// the program out of raw modes.
pub fn choose(
    modes: &[Mode],
    window: &ForegroundWindow,
    transcript: &str,
    shortcut: Option<&str>,
    legacy_rules: &[app_style::AppStyleRule],
) -> Option<ModeChoice> {
    if modes.is_empty() {
        return None;
    }
    let legacy = window
        .process
        .as_deref()
        .and_then(|p| app_style::chosen_style(p, legacy_rules));
    let pick = |matched_by, f: &dyn Fn(&ModeRule) -> bool| {
        modes
            .iter()
            .position(|m| {
                let blocked = legacy == Some(Style::Normal) && m.style == Style::Raw;
                !blocked && m.rules.iter().any(f)
            })
            .map(|index| ModeChoice {
                index,
                matched_by,
                trigger_words: 0,
            })
    };

    if let Some(shortcut) = shortcut {
        if let Some(c) = pick(
            MatchedBy::Shortcut,
            &|r| matches!(r, ModeRule::Shortcut(s) if s.eq_ignore_ascii_case(shortcut)),
        ) {
            return Some(c);
        }
    }

    let spoken = words(transcript);
    for (index, mode) in modes.iter().enumerate() {
        for rule in &mode.rules {
            if let ModeRule::TriggerWord(trigger) = rule {
                let trigger = words(trigger);
                if !trigger.is_empty()
                    && spoken.len() > trigger.len()
                    && spoken.starts_with(&trigger)
                {
                    return Some(ModeChoice {
                        index,
                        matched_by: MatchedBy::TriggerWord,
                        trigger_words: trigger.len(),
                    });
                }
            }
        }
    }

    if legacy == Some(Style::Raw) {
        if let Some(index) = modes.iter().position(|m| m.style == Style::Raw) {
            return Some(ModeChoice {
                index,
                matched_by: MatchedBy::LegacyStyle,
                trigger_words: 0,
            });
        }
    }
    if let Some(process) = &window.process {
        if let Some(c) = pick(
            MatchedBy::Program,
            &|r| matches!(r, ModeRule::Program(p) if same_program(p, process)),
        ) {
            return Some(c);
        }
    }
    if let Some(class) = &window.class {
        if let Some(c) = pick(
            MatchedBy::WindowClass,
            &|r| matches!(r, ModeRule::WindowClass(k) if k.eq_ignore_ascii_case(class)),
        ) {
            return Some(c);
        }
    }
    if let Some(title) = &window.title {
        let title = title.to_lowercase();
        if let Some(c) = pick(
            MatchedBy::Title,
            &|r| matches!(r, ModeRule::TitleContains(t) if !t.trim().is_empty() && title.contains(&t.trim().to_lowercase())),
        ) {
            return Some(c);
        }
    }
    let index = modes.iter().position(|m| m.id == "standard").unwrap_or(0);
    Some(ModeChoice {
        index,
        matched_by: MatchedBy::Default,
        trigger_words: 0,
    })
}

/// Remove the first `n` words (a trigger word) from `text`.
pub fn strip_leading_words(text: &str, n: usize) -> String {
    if n == 0 {
        return text.to_string();
    }
    let mut rest = text.trim_start();
    for _ in 0..n {
        rest = rest.trim_start();
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        rest = &rest[end..];
    }
    rest.trim_start_matches(|c: char| c.is_whitespace() || ",.:;".contains(c))
        .to_string()
}

/// The dictation settings a mode can override.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defaults {
    pub engine: String,
    pub model: String,
    pub language: String,
    pub polish: bool,
    pub format_numbers: bool,
    pub format_addresses: bool,
}

/// Settings for one dictation after the mode's overrides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub mode_id: String,
    pub mode_name: String,
    pub engine: String,
    pub model: String,
    pub language: String,
    pub style: Style,
    pub polish: bool,
    pub prompt: String,
    pub format_numbers: bool,
    pub format_addresses: bool,
    /// Empty unless the mode presses Enter after a trigger word.
    pub enter_triggers: Vec<String>,
}

pub fn resolve(mode: &Mode, defaults: &Defaults) -> Resolved {
    let raw = mode.style == Style::Raw;
    let pick = |s: &str, d: &str| {
        if s.trim().is_empty() {
            d.to_string()
        } else {
            s.trim().to_string()
        }
    };
    Resolved {
        mode_id: mode.id.clone(),
        mode_name: mode.name.clone(),
        engine: pick(&mode.provider, &defaults.engine),
        model: pick(&mode.model, &defaults.model),
        language: mode
            .language
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .unwrap_or(&defaults.language)
            .to_string(),
        style: mode.style,
        // Raw text is never polished.
        polish: !raw && mode.polish.unwrap_or(defaults.polish),
        prompt: mode.prompt.clone(),
        format_numbers: mode
            .format_numbers
            .unwrap_or(defaults.format_numbers && !raw),
        format_addresses: mode
            .format_addresses
            .unwrap_or(defaults.format_addresses && !raw),
        // "Press Enter" is never used in code editors and terminals.
        enter_triggers: if mode.output == Output::PasteAndEnter && !raw {
            mode.enter_triggers
                .iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        } else {
            Vec::new()
        },
    }
}

/// Base instruction for LLM polishing (layer 2).
pub const POLISH_BASE: &str = "Du retter dikteret dansk tekst. Ret grammatik og tegnsætning, \
fjern gentagelser og selvrettelser (behold den rettede udgave), og lav punktopstilling hvis \
teksten opremser flere ting. Bevar betydning, sprog og tone. Oversæt aldrig: engelske ord og \
fagudtryk i en dansk sætning bliver stående med engelsk stavning. Svar kun med den rettede tekst.";

/// The polishing prompt for a mode, with the dictionary's English words.
pub fn polish_prompt(mode_prompt: &str, english_terms: &[String]) -> String {
    let mut prompt = POLISH_BASE.to_string();
    if !mode_prompt.trim().is_empty() {
        prompt.push(' ');
        prompt.push_str(mode_prompt.trim());
    }
    if !english_terms.is_empty() {
        prompt.push_str(" Disse ord er engelske og skal stå præcis sådan: ");
        prompt.push_str(&english_terms.join(", "));
        prompt.push('.');
    }
    prompt
}

// ---------------------------------------------------------------------------
// Export and import
// ---------------------------------------------------------------------------

pub const MODES_FORMAT: &str = "tingskriver-modes";
pub const MODES_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModesFile {
    format: String,
    version: u32,
    modes: Vec<Mode>,
}

pub fn export_modes(modes: &[Mode]) -> Result<String, String> {
    serde_json::to_string_pretty(&ModesFile {
        format: MODES_FORMAT.to_string(),
        version: MODES_VERSION,
        modes: modes.to_vec(),
    })
    .map_err(|e| e.to_string())
}

pub fn import_modes(json: &str) -> Result<Vec<Mode>, String> {
    let file: ModesFile = serde_json::from_str(json)
        .map_err(|e| format!("Filen med tilstande kan ikke læses: {e}"))?;
    if file.format != MODES_FORMAT {
        return Err("Filen indeholder ikke tilstande fra Tingskriver".to_string());
    }
    if file.version > MODES_VERSION {
        return Err(format!(
            "Tilstandene er gemt med en nyere version af Tingskriver (format {}); opdatér programmet",
            file.version
        ));
    }
    validate(file.modes)
}

/// Trim names and ids and reject empty or duplicate modes.
pub fn validate(modes: Vec<Mode>) -> Result<Vec<Mode>, String> {
    let mut out: Vec<Mode> = Vec::with_capacity(modes.len());
    for mut mode in modes {
        mode.name = mode.name.trim().to_string();
        mode.id = mode.id.trim().to_lowercase();
        if mode.name.is_empty() {
            return Err("En tilstand mangler et navn".to_string());
        }
        if mode.id.is_empty() {
            mode.id = mode
                .name
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-");
        }
        if out.iter().any(|m| m.id == mode.id) {
            return Err(format!("Tilstanden \"{}\" findes to gange", mode.name));
        }
        if let Some(lang) = &mode.language {
            if !valid_language(lang) {
                return Err(format!(
                    "Ukendt sprogkode \"{lang}\" i tilstanden \"{}\"",
                    mode.name
                ));
            }
        }
        out.push(mode);
    }
    if out.is_empty() {
        return Err("Der skal være mindst én tilstand".to_string());
    }
    Ok(out)
}

/// ISO 639-1 style code ("da", "en") or empty.
pub fn valid_language(code: &str) -> bool {
    let code = code.trim();
    code.is_empty() || (code.len() == 2 && code.chars().all(|c| c.is_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(process: &str, title: &str) -> ForegroundWindow {
        ForegroundWindow {
            process: Some(process.into()),
            class: None,
            title: Some(title.into()),
        }
    }

    fn chosen(window: &ForegroundWindow, text: &str) -> String {
        let modes = builtin_modes();
        let c = choose(&modes, window, text, None, &app_style::default_rules()).unwrap();
        modes[c.index].id.clone()
    }

    #[test]
    fn builtin_modes_are_danish() {
        let modes = builtin_modes();
        let names: Vec<&str> = modes.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["Standard", "Formel", "Chat", "Kode/rå"]);
        assert!(validate(modes).is_ok());
    }

    #[test]
    fn outlook_gives_formel() {
        assert_eq!(
            chosen(
                &window("OUTLOOK.EXE", "Indbakke - mads@mahoje.dk - Outlook"),
                "hej anna"
            ),
            "formel"
        );
        assert_eq!(
            chosen(
                &window(
                    "C:\\Program Files\\Microsoft Office\\root\\Office16\\outlook.exe",
                    ""
                ),
                "hej"
            ),
            "formel"
        );
        // New Outlook and Outlook on the web in a browser.
        assert_eq!(chosen(&window("olk.exe", ""), "hej"), "formel");
        assert_eq!(
            chosen(&window("msedge.exe", "Mail - Mads - Outlook"), "hej"),
            "formel"
        );
    }

    #[test]
    fn programs_titles_and_default() {
        assert_eq!(
            chosen(&window("slack.exe", "Slack | general"), "hej"),
            "chat"
        );
        assert_eq!(
            chosen(&window("chrome.exe", "general - Mahope - Slack"), "hej"),
            "chat"
        );
        assert_eq!(
            chosen(&window("Code.exe", "main.rs - tingskriver"), "hej"),
            "kode"
        );
        assert_eq!(
            chosen(&window("notepad.exe", "Uden titel"), "hej"),
            "standard"
        );
        assert_eq!(chosen(&ForegroundWindow::default(), "hej"), "standard");
    }

    #[test]
    fn program_beats_title() {
        // Slack open in a window whose title mentions Outlook.
        assert_eq!(chosen(&window("slack.exe", "Outlook-tråd"), "hej"), "chat");
    }

    #[test]
    fn window_class_rule() {
        let mut modes = builtin_modes();
        modes[1]
            .rules
            .push(ModeRule::WindowClass("rctrl_renwnd32".into()));
        let w = ForegroundWindow {
            process: Some("ukendt.exe".into()),
            class: Some("rctrl_renwnd32".into()),
            title: None,
        };
        let c = choose(&modes, &w, "hej", None, &[]).unwrap();
        assert_eq!(
            (modes[c.index].id.as_str(), c.matched_by),
            ("formel", MatchedBy::WindowClass)
        );
    }

    #[test]
    fn trigger_word_and_shortcut() {
        // No built-in mode has a trigger word ("Formel 1 er sjovt" must stay text).
        assert_eq!(
            chosen(&window("notepad.exe", ""), "Formel 1 er sjovt"),
            "standard"
        );
        let mut modes = builtin_modes();
        modes[1].rules.push(ModeRule::TriggerWord("formel".into()));
        let c = choose(
            &modes,
            &window("notepad.exe", ""),
            "Formel, kære Anna",
            None,
            &[],
        )
        .unwrap();
        assert_eq!((modes[c.index].id.as_str(), c.trigger_words), ("formel", 1));
        assert_eq!(
            strip_leading_words("Formel, kære Anna", c.trigger_words),
            "kære Anna"
        );
        // The trigger word alone, or in the middle, is ordinary text.
        for text in ["formel", "det er formel tekst"] {
            let c = choose(&modes, &window("notepad.exe", ""), text, None, &[]).unwrap();
            assert_eq!(
                (modes[c.index].id.as_str(), c.trigger_words),
                ("standard", 0)
            );
        }

        let mut modes = builtin_modes();
        modes[2].rules.push(ModeRule::Shortcut("Ctrl+Win+C".into()));
        let c = choose(
            &modes,
            &window("OUTLOOK.EXE", ""),
            "hej",
            Some("ctrl+win+c"),
            &[],
        )
        .unwrap();
        assert_eq!(
            (modes[c.index].id.as_str(), c.matched_by),
            ("chat", MatchedBy::Shortcut)
        );
    }

    #[test]
    fn legacy_raw_programs_pick_code_mode() {
        let legacy = vec![app_style::AppStyleRule {
            process: "myeditor.exe".into(),
            style: Style::Raw,
        }];
        let modes = builtin_modes();
        let c = choose(&modes, &window("myeditor.exe", ""), "hej", None, &legacy).unwrap();
        assert_eq!(
            (modes[c.index].id.as_str(), c.matched_by),
            ("kode", MatchedBy::LegacyStyle)
        );
    }

    #[test]
    fn legacy_list_overrides_builtin_program_rules() {
        let modes = builtin_modes();
        let id = |process: &str, legacy: &[app_style::AppStyleRule]| {
            let c = choose(&modes, &window(process, ""), "hej", None, legacy).unwrap();
            modes[c.index].id.clone()
        };
        let mut legacy = app_style::default_rules();
        assert_eq!(id("Code.exe", &legacy), "kode");
        // VS Code set to normal in the old list.
        legacy[0].style = Style::Normal;
        assert_eq!(id("Code.exe", &legacy), "standard");
        // pwsh removed from the list.
        legacy.retain(|r| r.process != "pwsh.exe");
        assert_eq!(id("pwsh.exe", &legacy), "standard");
        // Editors only the modes know are unaffected.
        assert_eq!(id("zed.exe", &legacy), "kode");
        // Slack set to raw in the old list beats the Chat program rule.
        legacy.push(app_style::AppStyleRule {
            process: "slack.exe".into(),
            style: Style::Raw,
        });
        assert_eq!(id("slack.exe", &legacy), "kode");
    }

    fn defaults() -> Defaults {
        Defaults {
            engine: "auto".into(),
            model: "syv-transcribe".into(),
            language: "da".into(),
            polish: true,
            format_numbers: true,
            format_addresses: true,
        }
    }

    #[test]
    fn modes_override_settings() {
        let modes = builtin_modes();
        let standard = resolve(&modes[0], &defaults());
        assert_eq!(standard.language, "da");
        assert!(standard.polish && standard.format_numbers && standard.enter_triggers.is_empty());

        let code = resolve(&modes[3], &defaults());
        assert_eq!(code.style, Style::Raw);
        assert!(!code.polish && !code.format_numbers && !code.format_addresses);

        let chat = resolve(&modes[2], &defaults());
        assert_eq!(chat.enter_triggers, vec!["send", "send den", "send besked"]);

        let english = Mode {
            language: Some("en".into()),
            format_numbers: Some(false),
            provider: "local".into(),
            ..modes[0].clone()
        };
        let r = resolve(&english, &defaults());
        assert_eq!(
            (r.language.as_str(), r.format_numbers, r.engine.as_str()),
            ("en", false, "local")
        );
    }

    #[test]
    fn enter_is_never_pressed_in_code_mode() {
        let code = Mode {
            output: Output::PasteAndEnter,
            enter_triggers: vec!["send".into()],
            ..builtin_modes()[3].clone()
        };
        assert!(resolve(&code, &defaults()).enter_triggers.is_empty());
    }

    #[test]
    fn polish_prompt_never_translates_and_keeps_english_terms() {
        let prompt = polish_prompt("", &[]);
        assert!(prompt.contains("Oversæt aldrig"));
        let prompt = polish_prompt(
            &builtin_modes()[1].prompt,
            &["pull request".into(), "deploy".into()],
        );
        assert!(prompt.contains("formel"));
        assert!(prompt.ends_with("skal stå præcis sådan: pull request, deploy."));
    }

    #[test]
    fn export_import_round_trip() {
        let mut modes = builtin_modes();
        modes[0].language = Some("en".into());
        modes[1].rules.push(ModeRule::Shortcut("Ctrl+Win+F".into()));
        let json = export_modes(&modes).unwrap();
        assert!(json.contains("\"version\": 1"));
        assert_eq!(import_modes(&json).unwrap(), modes);
    }

    #[test]
    fn import_rejects_bad_files() {
        assert!(import_modes("[]").is_err());
        assert!(import_modes(r#"{"format":"andet","version":1,"modes":[]}"#).is_err());
        assert!(import_modes(r#"{"format":"tingskriver-modes","version":2,"modes":[]}"#).is_err());
        assert!(import_modes(r#"{"format":"tingskriver-modes","version":1,"modes":[]}"#).is_err());
        let dup = r#"{"format":"tingskriver-modes","version":1,"modes":[{"id":"a","name":"A"},{"id":"a","name":"B"}]}"#;
        assert!(import_modes(dup).is_err());
        let lang = r#"{"format":"tingskriver-modes","version":1,"modes":[{"id":"a","name":"A","language":"dansk"}]}"#;
        assert!(import_modes(lang).is_err());
        // Missing fields get defaults.
        let minimal =
            r#"{"format":"tingskriver-modes","version":1,"modes":[{"name":"Min tilstand"}]}"#;
        let modes = import_modes(minimal).unwrap();
        assert_eq!(
            (modes[0].id.as_str(), modes[0].style),
            ("min-tilstand", Style::Normal)
        );
    }
}
