//! Global shortcuts: what each one does, how they are stored, what
//! `RegisterHotKey` needs, and which ones collide.
//!
//! Dictation (hold to talk) needs both press and release and may be
//! modifiers only (Ctrl+Win), so the keyboard hook in `dictation` serves it
//! and its shortcut lives in the dictation settings. The other shortcuts fire
//! once per press and are registered with `RegisterHotKey` by the host
//! (`tingskriver-host`); they are stored here, in [`STORE`].
//!
//! [`check`] is the conflict check for the shortcut settings page: it
//! returns every problem with a Danish message, and [`Conflict::blocks`]
//! tells whether the shortcut can be saved at all.

use serde::{Deserialize, Serialize};

use crate::dictation::logic::hotkey::{vk, Chord};
use crate::host::AppHost;

/// Settings file (host store) holding [`ShortcutSettings`].
pub const STORE: &str = "shortcuts.json";
/// Key of [`ShortcutSettings`] in [`STORE`].
pub const KEY: &str = "shortcuts";

/// What a global shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutAction {
    /// Hold to dictate (the keyboard hook).
    Dictate,
    /// Start or stop a meeting recording.
    NewRecording,
    /// Open the assistant.
    Ask,
    /// Show the main window.
    OpenWindow,
}

impl ShortcutAction {
    /// The actions registered with `RegisterHotKey`.
    pub const REGISTERED: [ShortcutAction; 3] = [
        ShortcutAction::NewRecording,
        ShortcutAction::Ask,
        ShortcutAction::OpenWindow,
    ];

    /// Name in the UI.
    pub fn label(self) -> &'static str {
        match self {
            ShortcutAction::Dictate => "Diktér",
            ShortcutAction::NewRecording => "Ny optagelse",
            ShortcutAction::Ask => "Spørg",
            ShortcutAction::OpenWindow => "Åbn Tingskriver",
        }
    }

    /// Stable number used as the `RegisterHotKey` id.
    pub fn id(self) -> i32 {
        match self {
            ShortcutAction::Dictate => 1,
            ShortcutAction::NewRecording => 2,
            ShortcutAction::Ask => 3,
            ShortcutAction::OpenWindow => 4,
        }
    }

    pub fn from_id(id: i32) -> Option<ShortcutAction> {
        [
            ShortcutAction::Dictate,
            ShortcutAction::NewRecording,
            ShortcutAction::Ask,
            ShortcutAction::OpenWindow,
        ]
        .into_iter()
        .find(|a| a.id() == id)
    }
}

/// The shortcuts registered with `RegisterHotKey`. `None` means no shortcut.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShortcutSettings {
    pub new_recording: Option<String>,
    pub ask: Option<String>,
    pub open_window: Option<String>,
}

impl ShortcutSettings {
    pub fn get(&self, action: ShortcutAction) -> Option<&str> {
        let slot = match action {
            ShortcutAction::Dictate => return None,
            ShortcutAction::NewRecording => &self.new_recording,
            ShortcutAction::Ask => &self.ask,
            ShortcutAction::OpenWindow => &self.open_window,
        };
        slot.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }

    pub fn set(&mut self, action: ShortcutAction, shortcut: Option<String>) {
        let slot = match action {
            ShortcutAction::Dictate => return,
            ShortcutAction::NewRecording => &mut self.new_recording,
            ShortcutAction::Ask => &mut self.ask,
            ShortcutAction::OpenWindow => &mut self.open_window,
        };
        *slot = shortcut;
    }

    /// The shortcuts that parse, in [`ShortcutAction::REGISTERED`] order.
    pub fn chords(&self) -> Vec<(ShortcutAction, Chord)> {
        ShortcutAction::REGISTERED
            .into_iter()
            .filter_map(|action| Some((action, Chord::parse(self.get(action)?).ok()?)))
            .collect()
    }

    /// Read from the host's [`STORE`]; defaults when missing or unreadable.
    pub fn load(host: &dyn AppHost) -> ShortcutSettings {
        match host.read_setting(STORE, KEY) {
            Ok(Some(value)) => serde_json::from_value(value).unwrap_or_else(|e| {
                log::warn!("Shortcut settings unreadable, using defaults: {e}");
                ShortcutSettings::default()
            }),
            Ok(None) => ShortcutSettings::default(),
            Err(e) => {
                log::warn!("Shortcut settings could not be read: {e}");
                ShortcutSettings::default()
            }
        }
    }

    pub fn save(&self, host: &dyn AppHost) -> Result<(), String> {
        let value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        host.write_setting(STORE, KEY, value)
    }
}

/// `RegisterHotKey` modifier flags.
pub mod flags {
    pub const MOD_ALT: u32 = 0x0001;
    pub const MOD_CONTROL: u32 = 0x0002;
    pub const MOD_SHIFT: u32 = 0x0004;
    pub const MOD_WIN: u32 = 0x0008;
    /// Holding the shortcut does not repeat it.
    pub const MOD_NOREPEAT: u32 = 0x4000;
}

/// Arguments for `RegisterHotKey`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Registration {
    pub modifiers: u32,
    pub vk: u32,
}

/// What `RegisterHotKey` needs for `chord`. Shortcuts of modifiers only
/// cannot be registered that way (only the keyboard hook sees them).
pub fn registration(chord: Chord) -> Result<Registration, String> {
    let key = chord.key().ok_or_else(|| {
        format!(
            "{} har ingen almindelig tast. Kun Diktér kan bruge en genvej af Ctrl, Win, Alt og Skift alene",
            chord.label()
        )
    })?;
    let m = chord.modifiers();
    let mut modifiers = flags::MOD_NOREPEAT;
    for (on, flag) in [
        (m.alt, flags::MOD_ALT),
        (m.ctrl, flags::MOD_CONTROL),
        (m.shift, flags::MOD_SHIFT),
        (m.win, flags::MOD_WIN),
    ] {
        if on {
            modifiers |= flag;
        }
    }
    Ok(Registration { modifiers, vk: key })
}

/// What is wrong with a shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// The text is not a shortcut.
    Invalid(String),
    /// Only Ctrl, Win, Alt and Shift, for an action that needs a key.
    ModifiersOnly,
    /// Another action has the same shortcut.
    SameAs(ShortcutAction),
    /// Pressing it starts with the dictation shortcut, so dictation starts
    /// for a moment and is cancelled.
    StartsDictation,
    /// Windows uses this shortcut itself (`what` says for what).
    Windows(&'static str),
    /// It would type text (a letter without Ctrl, Alt or Win).
    TypesText,
    /// Most programs use it; registered globally it stops working there.
    Common(&'static str),
    /// A key without modifiers; it stops working in every other program.
    NoModifier,
    /// Another program has registered it already (found when registering).
    TakenByOtherProgram,
}

/// A problem with the shortcut of `action`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub action: ShortcutAction,
    /// The shortcut as the user wrote it, or its canonical label.
    pub shortcut: String,
    pub kind: ConflictKind,
}

impl Conflict {
    /// The shortcut cannot be used; the settings page should not save it.
    /// The others are warnings.
    pub fn blocks(&self) -> bool {
        matches!(
            self.kind,
            ConflictKind::Invalid(_)
                | ConflictKind::ModifiersOnly
                | ConflictKind::SameAs(_)
                | ConflictKind::Windows(_)
                | ConflictKind::TypesText
                | ConflictKind::TakenByOtherProgram
        )
    }

    /// Danish text for the settings page and the tray tooltip.
    pub fn message(&self) -> String {
        let s = &self.shortcut;
        match &self.kind {
            ConflictKind::Invalid(reason) => reason.clone(),
            ConflictKind::ModifiersOnly => format!(
                "{s} har ingen almindelig tast. Kun Diktér kan bruge Ctrl, Win, Alt og Skift alene"
            ),
            ConflictKind::SameAs(other) => format!("{s} bruges allerede til {}", other.label()),
            ConflictKind::StartsDictation => format!(
                "{s} begynder med genvejen til Diktér. Dikteringen starter et øjeblik og stopper igen, når du trykker den"
            ),
            ConflictKind::Windows(what) => format!("Windows bruger selv {s} til {what}"),
            ConflictKind::TypesText => format!("{s} skriver bare tekst. Vælg en genvej med Ctrl, Alt eller Win"),
            ConflictKind::Common(what) => format!(
                "{s} bruges til {what} i de fleste programmer og virker ikke længere dér, mens Tingskriver kører"
            ),
            ConflictKind::NoModifier => format!(
                "{s} uden Ctrl, Alt eller Win virker ikke længere i andre programmer, mens Tingskriver kører"
            ),
            ConflictKind::TakenByOtherProgram => format!("Et andet program bruger allerede {s}"),
        }
    }
}

const fn letter(c: char) -> u32 {
    c as u32
}

/// Ctrl+Win shortcuts Windows uses itself.
const WINDOWS_CTRL_WIN: &[(u32, &str)] = &[
    (letter('D'), "at oprette et nyt skrivebord"),
    (vk::F1 + 3, "at lukke skrivebordet"),
    (letter('C'), "farvefiltre"),
    (letter('O'), "skærmtastaturet"),
    (letter('N'), "indstillinger for Oplæser"),
    (letter('S'), "talegenkendelse"),
    (letter('Q'), "Hurtig hjælp"),
    (letter('F'), "at søge efter computere"),
];

/// Ctrl+key shortcuts nearly every program uses.
const COMMON_CTRL: &[(u32, &str)] = &[
    (letter('A'), "at markere alt"),
    (letter('C'), "at kopiere"),
    (letter('V'), "at indsætte"),
    (letter('X'), "at klippe"),
    (letter('Z'), "at fortryde"),
    (letter('Y'), "at gentage"),
    (letter('S'), "at gemme"),
    (letter('F'), "at søge"),
    (letter('P'), "at udskrive"),
    (letter('N'), "et nyt dokument eller vindue"),
    (letter('O'), "at åbne en fil"),
    (letter('W'), "at lukke en fane"),
    (letter('T'), "en ny fane"),
    (letter('R'), "at genindlæse"),
    (letter('B'), "fed skrift"),
    (letter('I'), "kursiv"),
    (letter('U'), "understregning"),
];

/// Problems of one chord on its own (not against the other shortcuts).
fn own_problem(action: ShortcutAction, chord: Chord) -> Option<ConflictKind> {
    let m = chord.modifiers();
    let Some(key) = chord.key() else {
        return (action != ShortcutAction::Dictate).then_some(ConflictKind::ModifiersOnly);
    };
    let is_function_key = (vk::F1..vk::F1 + 24).contains(&key);
    if !m.ctrl && !m.alt && !m.win {
        if !is_function_key {
            return Some(ConflictKind::TypesText);
        }
        return (!m.shift).then_some(ConflictKind::NoModifier);
    }
    if m.win && !m.ctrl && !m.alt {
        // Windows reserves Win plus a letter, digit or Space (with or without Shift).
        return Some(ConflictKind::Windows(if key == vk::SPACE {
            "at skifte tastatursprog"
        } else if (letter('0')..=letter('9')).contains(&key) {
            "at åbne programmer på proceslinjen"
        } else if key == letter('L') {
            "at låse computeren"
        } else if key == letter('D') {
            "at vise skrivebordet"
        } else if key == letter('E') {
            "Stifinder"
        } else {
            "sine egne genveje"
        }));
    }
    if m.win && m.ctrl && !m.alt && !m.shift {
        if let Some((_, what)) = WINDOWS_CTRL_WIN.iter().find(|(k, _)| *k == key) {
            return Some(ConflictKind::Windows(what));
        }
    }
    if m.alt && !m.ctrl && !m.win && !m.shift {
        if key == vk::F1 + 3 {
            return Some(ConflictKind::Windows("at lukke vinduet"));
        }
        if key == vk::SPACE {
            return Some(ConflictKind::Windows("vinduesmenuen"));
        }
    }
    if m.ctrl && !m.alt && !m.win && !m.shift {
        if let Some((_, what)) = COMMON_CTRL.iter().find(|(k, _)| *k == key) {
            return Some(ConflictKind::Common(what));
        }
    }
    None
}

/// Every problem with a set of parsed shortcuts. `Dictate` (if present) is
/// served by the keyboard hook; the others are registered.
pub fn find_conflicts(bindings: &[(ShortcutAction, Chord)]) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    let dictation = bindings
        .iter()
        .find(|(a, _)| *a == ShortcutAction::Dictate)
        .map(|(_, c)| *c);
    for (i, &(action, chord)) in bindings.iter().enumerate() {
        let conflict = |kind| Conflict {
            action,
            shortcut: chord.label(),
            kind,
        };
        if let Some(kind) = own_problem(action, chord) {
            conflicts.push(conflict(kind));
        }
        if let Some(&(other, _)) = bindings[..i].iter().find(|(_, c)| *c == chord) {
            conflicts.push(conflict(ConflictKind::SameAs(other)));
            continue;
        }
        if let Some(dictation) = dictation.filter(|_| action != ShortcutAction::Dictate) {
            let d = dictation.modifiers();
            let m = chord.modifiers();
            // The hook sees the dictation modifiers held before the rest of
            // the shortcut is pressed.
            let contains = (!d.ctrl || m.ctrl) && (!d.win || m.win) && (!d.alt || m.alt) && (!d.shift || m.shift);
            if dictation.key().is_none() && contains {
                conflicts.push(conflict(ConflictKind::StartsDictation));
            }
        }
    }
    conflicts
}

/// The conflict check for the settings page: the dictation shortcut (`None`
/// when dictation is off) plus the registered shortcuts, as written.
pub fn check(dictation: Option<&str>, settings: &ShortcutSettings) -> Vec<Conflict> {
    let mut problems = Vec::new();
    let mut bindings = Vec::new();
    let texts = dictation
        .map(|d| (ShortcutAction::Dictate, d))
        .into_iter()
        .chain(ShortcutAction::REGISTERED.into_iter().filter_map(|a| Some((a, settings.get(a)?))));
    for (action, text) in texts {
        match Chord::parse(text) {
            Ok(chord) => bindings.push((action, chord)),
            Err(reason) => problems.push(Conflict {
                action,
                shortcut: text.to_string(),
                kind: ConflictKind::Invalid(reason),
            }),
        }
    }
    problems.extend(find_conflicts(&bindings));
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> Chord {
        Chord::parse(text).unwrap()
    }

    fn kinds(dictation: Option<&str>, settings: &ShortcutSettings) -> Vec<(ShortcutAction, ConflictKind)> {
        check(dictation, settings).into_iter().map(|c| (c.action, c.kind)).collect()
    }

    fn settings(new_recording: Option<&str>, ask: Option<&str>, open_window: Option<&str>) -> ShortcutSettings {
        ShortcutSettings {
            new_recording: new_recording.map(String::from),
            ask: ask.map(String::from),
            open_window: open_window.map(String::from),
        }
    }

    #[test]
    fn ids_round_trip() {
        for action in [ShortcutAction::Dictate, ShortcutAction::NewRecording, ShortcutAction::Ask, ShortcutAction::OpenWindow] {
            assert_eq!(ShortcutAction::from_id(action.id()), Some(action));
        }
        assert_eq!(ShortcutAction::from_id(0), None);
    }

    #[test]
    fn registration_flags() {
        assert_eq!(
            registration(chord("Ctrl+Alt+N")).unwrap(),
            Registration {
                modifiers: flags::MOD_CONTROL | flags::MOD_ALT | flags::MOD_NOREPEAT,
                vk: 'N' as u32
            }
        );
        assert_eq!(
            registration(chord("Ctrl+Win+Space")).unwrap(),
            Registration {
                modifiers: flags::MOD_CONTROL | flags::MOD_WIN | flags::MOD_NOREPEAT,
                vk: vk::SPACE
            }
        );
        assert_eq!(registration(chord("Shift+F9")).unwrap().modifiers, flags::MOD_SHIFT | flags::MOD_NOREPEAT);
        let err = registration(chord("Ctrl+Win")).unwrap_err();
        assert!(err.contains("Kun Diktér"), "{err}");
    }

    #[test]
    fn good_defaults_have_no_problems() {
        assert!(kinds(Some("Ctrl+Win"), &ShortcutSettings::default()).is_empty());
        assert!(kinds(Some("Ctrl+Win"), &settings(Some("Ctrl+Alt+N"), Some("Ctrl+Alt+Space"), Some("Ctrl+Alt+T"))).is_empty());
    }

    #[test]
    fn same_shortcut_twice_blocks() {
        let found = check(Some("Ctrl+Win"), &settings(Some("Ctrl+Alt+N"), None, Some("alt+ctrl+n")));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].action, ShortcutAction::OpenWindow);
        assert_eq!(found[0].kind, ConflictKind::SameAs(ShortcutAction::NewRecording));
        assert!(found[0].blocks());
        assert_eq!(found[0].message(), "Ctrl+Alt+N bruges allerede til Ny optagelse");

        // The same shortcut as dictation blocks too.
        assert_eq!(
            kinds(Some("Ctrl+Alt+D"), &settings(Some("Ctrl+Alt+D"), None, None)),
            vec![(ShortcutAction::NewRecording, ConflictKind::SameAs(ShortcutAction::Dictate))]
        );
    }

    #[test]
    fn shortcuts_containing_the_dictation_modifiers_warn() {
        let found = check(Some("Ctrl+Win"), &settings(None, Some("Ctrl+Win+Space"), Some("Ctrl+Alt+Win+O")));
        let seen: Vec<_> = found.iter().map(|c| (c.action, c.kind.clone(), c.blocks())).collect();
        assert_eq!(
            seen,
            vec![
                (ShortcutAction::Ask, ConflictKind::StartsDictation, false),
                (ShortcutAction::OpenWindow, ConflictKind::StartsDictation, false),
            ]
        );
        // Dictation with its own key is not started by other shortcuts.
        assert!(kinds(Some("Ctrl+Alt+D"), &settings(None, Some("Ctrl+Alt+Space"), None)).is_empty());
        // Nor when dictation is off.
        assert!(kinds(None, &settings(None, Some("Ctrl+Win+Space"), None)).is_empty());
    }

    #[test]
    fn windows_shortcuts_block() {
        for text in ["Win+L", "Win+E", "Win+D", "Win+Shift+S", "Win+5", "Win+Space", "Ctrl+Win+D", "Alt+F4", "Alt+Space"] {
            let found = check(None, &settings(Some(text), None, None));
            assert_eq!(found.len(), 1, "{text}");
            assert!(matches!(found[0].kind, ConflictKind::Windows(_)), "{text}: {:?}", found[0].kind);
            assert!(found[0].blocks());
            assert!(found[0].message().starts_with("Windows bruger selv"), "{}", found[0].message());
        }
        assert_eq!(
            check(None, &settings(Some("Win+L"), None, None))[0].message(),
            "Windows bruger selv Win+L til at låse computeren"
        );
        // Ctrl+Win with a key Windows does not use is fine.
        assert!(check(None, &settings(Some("Ctrl+Win+R"), None, None)).is_empty());
    }

    #[test]
    fn text_keys_block_and_lone_function_keys_warn() {
        for text in ["A", "Shift+A", "Space", "7"] {
            let found = check(None, &settings(Some(text), None, None));
            assert_eq!(found[0].kind, ConflictKind::TypesText, "{text}");
            assert!(found[0].blocks());
        }
        let found = check(None, &settings(Some("F9"), None, None));
        assert_eq!(found[0].kind, ConflictKind::NoModifier);
        assert!(!found[0].blocks());
        assert!(check(None, &settings(Some("Shift+F9"), None, None)).is_empty());
    }

    #[test]
    fn common_editing_shortcuts_warn() {
        let found = check(None, &settings(Some("Ctrl+C"), None, None));
        assert_eq!(found[0].kind, ConflictKind::Common("at kopiere"));
        assert!(!found[0].blocks());
        assert_eq!(
            found[0].message(),
            "Ctrl+C bruges til at kopiere i de fleste programmer og virker ikke længere dér, mens Tingskriver kører"
        );
        assert!(check(None, &settings(Some("Ctrl+Shift+C"), None, None)).is_empty());
    }

    #[test]
    fn modifier_only_and_invalid_shortcuts_block() {
        let found = check(Some("Ctrl+Win"), &settings(Some("Ctrl+Alt"), Some("Ctrl+Hund"), None));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].action, ShortcutAction::Ask);
        assert!(matches!(found[0].kind, ConflictKind::Invalid(_)));
        assert_eq!(found[0].message(), "Ukendt tast i genvejen: Hund");
        assert_eq!(found[1].action, ShortcutAction::NewRecording);
        assert_eq!(found[1].kind, ConflictKind::ModifiersOnly);
        assert!(found.iter().all(Conflict::blocks));
        // Dictation itself may be modifiers only.
        assert!(check(Some("Ctrl+Alt"), &ShortcutSettings::default()).is_empty());
    }

    #[test]
    fn taken_by_other_program_message() {
        let c = Conflict {
            action: ShortcutAction::Ask,
            shortcut: "Ctrl+Alt+Space".into(),
            kind: ConflictKind::TakenByOtherProgram,
        };
        assert!(c.blocks());
        assert_eq!(c.message(), "Et andet program bruger allerede Ctrl+Alt+Space");
    }

    #[test]
    fn settings_round_trip_and_skip_blank() {
        let s = settings(Some("Ctrl+Alt+N"), Some("  "), None);
        assert_eq!(s.get(ShortcutAction::Ask), None);
        assert_eq!(s.chords(), vec![(ShortcutAction::NewRecording, chord("Ctrl+Alt+N"))]);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["newRecording"], "Ctrl+Alt+N");
        assert_eq!(serde_json::from_value::<ShortcutSettings>(json).unwrap(), s);
        // Missing fields fall back to none.
        let partial: ShortcutSettings = serde_json::from_str(r#"{"ask":"Ctrl+Alt+Space"}"#).unwrap();
        assert_eq!(partial, settings(None, Some("Ctrl+Alt+Space"), None));
        let mut s = ShortcutSettings::default();
        s.set(ShortcutAction::OpenWindow, Some("Ctrl+Alt+T".into()));
        s.set(ShortcutAction::Dictate, Some("ignored".into()));
        assert_eq!(s, settings(None, None, Some("Ctrl+Alt+T")));
    }
}
