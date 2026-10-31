//! What the tray shows: icon, tooltip and menu for the current status.
//!
//! Platform independent; the Windows tray in `win` only copies the result.

/// The meeting recording.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Recording {
    #[default]
    Idle,
    Recording,
    Paused,
}

/// The dictation service.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Dictation {
    /// Not started yet (the service starts in the background).
    #[default]
    Starting,
    /// The shortcut works; holds its label, e.g. "Ctrl+Win".
    Ready(String),
    /// The microphone is open.
    Listening(String),
    /// Turned off in the settings.
    Off,
    /// The shortcut could not be set up.
    Failed(String),
}

/// Everything the tray reflects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayStatus {
    pub recording: Recording,
    pub dictation: Dictation,
    /// Problems with the registered shortcuts, as Danish messages.
    pub shortcut_problems: Vec<String>,
    /// Tingskriver runs as administrator.
    pub elevated: bool,
}

/// The coloured dot on the tray icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconState {
    Idle,
    Recording,
    Paused,
    Problem,
}

/// One entry of the tray menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MenuEntry {
    NewRecording,
    Dictate,
    Ask,
    Open,
    Quit,
}

impl MenuEntry {
    /// Menu order; a separator goes before `Open`.
    pub const ALL: [MenuEntry; 5] = [
        MenuEntry::NewRecording,
        MenuEntry::Dictate,
        MenuEntry::Ask,
        MenuEntry::Open,
        MenuEntry::Quit,
    ];
}

/// Text and enabled state of a menu entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuLabel {
    pub text: String,
    pub enabled: bool,
}

/// Windows cuts tray tooltips at 127 characters.
pub const TOOLTIP_MAX: usize = 127;

impl TrayStatus {
    pub fn icon(&self) -> IconState {
        match self.recording {
            Recording::Recording => IconState::Recording,
            Recording::Paused => IconState::Paused,
            Recording::Idle if self.problem().is_some() => IconState::Problem,
            Recording::Idle => IconState::Idle,
        }
    }

    /// The most important thing that does not work, if any.
    fn problem(&self) -> Option<String> {
        if self.elevated {
            return Some(
                "Kører som administrator, så genveje og indsættelse virker ikke i alle programmer"
                    .into(),
            );
        }
        if let Dictation::Failed(reason) = &self.dictation {
            return Some(reason.clone());
        }
        self.shortcut_problems.first().cloned()
    }

    pub fn tooltip(&self) -> String {
        let headline = match (&self.recording, &self.dictation) {
            (Recording::Recording, _) => "Tingskriver optager".to_string(),
            (Recording::Paused, _) => "Tingskriver: optagelsen er sat på pause".to_string(),
            (Recording::Idle, Dictation::Listening(shortcut)) => {
                format!("Tingskriver lytter. Tryk {shortcut} for at indsætte")
            }
            (Recording::Idle, Dictation::Ready(shortcut)) => {
                format!("Tingskriver\nHold {shortcut} for at diktere")
            }
            (Recording::Idle, _) => "Tingskriver".to_string(),
        };
        let text = match self.problem() {
            Some(problem) => format!("{headline}\n{problem}"),
            None => headline,
        };
        truncate(&text, TOOLTIP_MAX)
    }

    pub fn menu(&self, entry: MenuEntry) -> MenuLabel {
        let label = |text: &str, enabled: bool| MenuLabel {
            text: text.to_string(),
            enabled,
        };
        match entry {
            MenuEntry::NewRecording => match self.recording {
                Recording::Idle => label("Ny optagelse", true),
                Recording::Recording | Recording::Paused => label("Stop optagelse", true),
            },
            // While listening, the shortcut inserts the text where the cursor
            // is; the menu would insert it into the menu's own window.
            MenuEntry::Dictate => match &self.dictation {
                Dictation::Ready(_) => label("Diktér", true),
                Dictation::Listening(shortcut) => {
                    label(&format!("Tryk {shortcut} for at indsætte"), false)
                }
                _ => label("Diktér", false),
            },
            MenuEntry::Ask => label("Spørg (kommer senere)", false),
            MenuEntry::Open => label("Åbn Tingskriver", true),
            MenuEntry::Quit => label("Afslut", true),
        }
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> TrayStatus {
        TrayStatus {
            dictation: Dictation::Ready("Ctrl+Win".into()),
            ..TrayStatus::default()
        }
    }

    #[test]
    fn idle_with_dictation_ready() {
        let s = ready();
        assert_eq!(s.icon(), IconState::Idle);
        assert_eq!(s.tooltip(), "Tingskriver\nHold Ctrl+Win for at diktere");
        assert_eq!(s.menu(MenuEntry::NewRecording).text, "Ny optagelse");
        assert!(s.menu(MenuEntry::Dictate).enabled);
        assert!(!s.menu(MenuEntry::Ask).enabled);
        assert_eq!(s.menu(MenuEntry::Open).text, "Åbn Tingskriver");
        assert_eq!(s.menu(MenuEntry::Quit).text, "Afslut");
    }

    #[test]
    fn recording_changes_icon_tooltip_and_menu() {
        let s = TrayStatus {
            recording: Recording::Recording,
            ..ready()
        };
        assert_eq!(s.icon(), IconState::Recording);
        assert_eq!(s.tooltip(), "Tingskriver optager");
        assert_eq!(s.menu(MenuEntry::NewRecording).text, "Stop optagelse");

        let s = TrayStatus {
            recording: Recording::Paused,
            ..ready()
        };
        assert_eq!(s.icon(), IconState::Paused);
        assert_eq!(s.tooltip(), "Tingskriver: optagelsen er sat på pause");
    }

    #[test]
    fn listening_disables_the_menu_entry() {
        let s = TrayStatus {
            dictation: Dictation::Listening("Ctrl+Win".into()),
            ..TrayStatus::default()
        };
        assert_eq!(
            s.tooltip(),
            "Tingskriver lytter. Tryk Ctrl+Win for at indsætte"
        );
        assert_eq!(
            s.menu(MenuEntry::Dictate),
            MenuLabel {
                text: "Tryk Ctrl+Win for at indsætte".into(),
                enabled: false
            }
        );
        for d in [
            Dictation::Starting,
            Dictation::Off,
            Dictation::Failed("x".into()),
        ] {
            let s = TrayStatus {
                dictation: d,
                ..TrayStatus::default()
            };
            assert!(!s.menu(MenuEntry::Dictate).enabled);
        }
    }

    #[test]
    fn problems_show_in_tooltip_and_icon() {
        let s = TrayStatus {
            shortcut_problems: vec!["Et andet program bruger allerede Ctrl+Alt+N".into()],
            ..ready()
        };
        assert_eq!(s.icon(), IconState::Problem);
        assert_eq!(
            s.tooltip(),
            "Tingskriver\nHold Ctrl+Win for at diktere\nEt andet program bruger allerede Ctrl+Alt+N"
        );
        // Recording wins over a problem in the icon.
        let s = TrayStatus {
            recording: Recording::Recording,
            ..s
        };
        assert_eq!(s.icon(), IconState::Recording);

        let s = TrayStatus {
            elevated: true,
            ..ready()
        };
        assert!(s.tooltip().contains("administrator"), "{}", s.tooltip());
    }

    #[test]
    fn tooltip_fits_windows() {
        let s = TrayStatus {
            dictation: Dictation::Failed("ø".repeat(400)),
            ..TrayStatus::default()
        };
        let tip = s.tooltip();
        assert_eq!(tip.chars().count(), TOOLTIP_MAX);
        assert!(tip.ends_with('…'));
    }
}
