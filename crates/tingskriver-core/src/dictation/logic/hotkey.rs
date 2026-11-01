//! Shortcut parsing and the per-key tracker used inside the keyboard hook.
//!
//! The tracker only knows virtual-key codes and up/down. It never stores
//! which keys were typed; it keeps a bit set of held modifiers and whether
//! the configured shortcut is currently held.

/// Windows virtual-key codes used by the tracker.
pub mod vk {
    pub const SHIFT: u32 = 0x10;
    pub const CONTROL: u32 = 0x11;
    pub const MENU: u32 = 0x12;
    pub const LWIN: u32 = 0x5B;
    pub const RWIN: u32 = 0x5C;
    pub const LSHIFT: u32 = 0xA0;
    pub const RSHIFT: u32 = 0xA1;
    pub const LCONTROL: u32 = 0xA2;
    pub const RCONTROL: u32 = 0xA3;
    pub const LMENU: u32 = 0xA4;
    pub const RMENU: u32 = 0xA5;
    pub const SPACE: u32 = 0x20;
    pub const F1: u32 = 0x70;
}

const MOD_CTRL: u8 = 1;
const MOD_WIN: u8 = 2;
const MOD_SHIFT: u8 = 4;
const MOD_ALT: u8 = 8;

fn modifier_bit(vk_code: u32) -> Option<u8> {
    match vk_code {
        vk::CONTROL | vk::LCONTROL | vk::RCONTROL => Some(MOD_CTRL),
        vk::LWIN | vk::RWIN => Some(MOD_WIN),
        vk::SHIFT | vk::LSHIFT | vk::RSHIFT => Some(MOD_SHIFT),
        vk::MENU | vk::LMENU | vk::RMENU => Some(MOD_ALT),
        _ => None,
    }
}

/// The modifiers of a [`Chord`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub win: bool,
    pub alt: bool,
    pub shift: bool,
}

/// A shortcut: a set of modifiers plus at most one ordinary key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    modifiers: u8,
    key: Option<u32>,
}

impl Chord {
    pub const DEFAULT: &'static str = "Ctrl+Win";

    /// Parse "Ctrl+Win", "Ctrl+Alt+Space", "F9" and similar.
    pub fn parse(text: &str) -> Result<Chord, String> {
        let mut modifiers = 0u8;
        let mut key = None;
        for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            let lower = part.to_lowercase();
            let bit = match lower.as_str() {
                "ctrl" | "control" | "ctl" => Some(MOD_CTRL),
                "win" | "windows" | "super" | "meta" => Some(MOD_WIN),
                "shift" | "skift" => Some(MOD_SHIFT),
                "alt" => Some(MOD_ALT),
                _ => None,
            };
            if let Some(bit) = bit {
                modifiers |= bit;
                continue;
            }
            if key.is_some() {
                return Err(format!("Genvejen kan kun have én almindelig tast: {text}"));
            }
            key = Some(parse_key(&lower).ok_or_else(|| format!("Ukendt tast i genvejen: {part}"))?);
        }
        if modifiers == 0 && key.is_none() {
            return Err("Genvejen er tom".to_string());
        }
        if key.is_none() && modifiers.count_ones() < 2 {
            return Err("En genvej uden almindelig tast skal have mindst to af Ctrl, Win, Alt og Skift".to_string());
        }
        Ok(Chord { modifiers, key })
    }

    pub fn uses_win(&self) -> bool {
        self.modifiers & MOD_WIN != 0
    }

    /// Which modifiers the shortcut holds.
    pub fn modifiers(&self) -> Modifiers {
        Modifiers {
            ctrl: self.modifiers & MOD_CTRL != 0,
            win: self.modifiers & MOD_WIN != 0,
            alt: self.modifiers & MOD_ALT != 0,
            shift: self.modifiers & MOD_SHIFT != 0,
        }
    }

    /// Virtual-key code of the ordinary key, `None` for a shortcut of
    /// modifiers only (like Ctrl+Win).
    pub fn key(&self) -> Option<u32> {
        self.key
    }

    /// Canonical text form, e.g. "Ctrl+Win".
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (bit, name) in [(MOD_CTRL, "Ctrl"), (MOD_WIN, "Win"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift")] {
            if self.modifiers & bit != 0 {
                parts.push(name.to_string());
            }
        }
        if let Some(key) = self.key {
            parts.push(key_name(key));
        }
        parts.join("+")
    }

    fn is_part(&self, vk_code: u32) -> bool {
        match modifier_bit(vk_code) {
            Some(bit) => self.modifiers & bit != 0,
            None => self.key == Some(vk_code),
        }
    }
}

fn parse_key(lower: &str) -> Option<u32> {
    match lower {
        "space" | "mellemrum" => return Some(vk::SPACE),
        _ => {}
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(vk::F1 + n - 1);
        }
    }
    let mut chars = lower.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c.to_ascii_uppercase() as u32),
        _ => None,
    }
}

fn key_name(vk_code: u32) -> String {
    match vk_code {
        vk::SPACE => "Space".to_string(),
        c if (vk::F1..vk::F1 + 24).contains(&c) => format!("F{}", c - vk::F1 + 1),
        c => char::from_u32(c).map(|ch| ch.to_string()).unwrap_or_else(|| format!("0x{c:02X}")),
    }
}

/// What the shortcut did, as seen by the hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChordSignal {
    Pressed,
    Released,
    /// Another key was pressed while the shortcut was held (e.g. Ctrl+Win+D).
    Interrupted,
}

/// What the hook should do with the current key event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HookDecision {
    pub signal: Option<ChordSignal>,
    /// Do not pass the event on to other programs.
    pub swallow: bool,
    /// Send a masking key and then this Win key-up, so the Start menu stays shut.
    pub inject_win_release: Option<u32>,
}

/// Tracks modifier state for one shortcut. Lives in the hook thread.
#[derive(Debug)]
pub struct ChordTracker {
    chord: Chord,
    held: u8,
    key_held: bool,
    active: bool,
    swallow_key_up: bool,
    /// The Win key must be masked when it is released.
    mask_win: bool,
}

impl ChordTracker {
    pub fn new(chord: Chord) -> Self {
        ChordTracker {
            chord,
            held: 0,
            key_held: false,
            active: false,
            swallow_key_up: false,
            mask_win: false,
        }
    }

    pub fn set_chord(&mut self, chord: Chord) {
        *self = ChordTracker::new(chord);
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    fn matches(&self) -> bool {
        self.held == self.chord.modifiers && self.chord.key.map_or(true, |_| self.key_held)
    }

    /// Forget modifiers that are no longer physically down. The hook never
    /// sees some key-ups (Win+L, the secure desktop, UAC prompts), and a
    /// modifier stuck in `held` would let e.g. Ctrl alone match Ctrl+Win.
    /// `is_down` reports the physical state of a virtual-key code. Returns
    /// `Interrupted` if an active shortcut lost one of its modifiers.
    pub fn resync(&mut self, is_down: impl Fn(u32) -> bool) -> Option<ChordSignal> {
        let physical = [
            (MOD_CTRL, is_down(vk::CONTROL)),
            (MOD_WIN, is_down(vk::LWIN) || is_down(vk::RWIN)),
            (MOD_SHIFT, is_down(vk::SHIFT)),
            (MOD_ALT, is_down(vk::MENU)),
        ];
        for (bit, down) in physical {
            if self.held & bit != 0 && !down {
                self.held &= !bit;
                if bit == MOD_WIN {
                    self.mask_win = false;
                }
            }
        }
        if self.active && self.held != self.chord.modifiers {
            self.active = false;
            return Some(ChordSignal::Interrupted);
        }
        None
    }

    pub fn key_down(&mut self, vk_code: u32) -> HookDecision {
        let mut decision = HookDecision::default();
        let bit = modifier_bit(vk_code);
        let repeat = match bit {
            Some(bit) => self.held & bit != 0,
            None => self.chord.key == Some(vk_code) && self.key_held,
        };
        if repeat {
            // Auto-repeat while held: keep swallowing the chord key.
            decision.swallow = self.active && bit.is_none();
            return decision;
        }
        match bit {
            Some(bit) => self.held |= bit,
            None if self.chord.key == Some(vk_code) => self.key_held = true,
            None => {}
        }

        if self.active {
            if !self.chord.is_part(vk_code) {
                self.active = false;
                decision.signal = Some(ChordSignal::Interrupted);
            }
            return decision;
        }

        if self.chord.is_part(vk_code) && self.matches() {
            self.active = true;
            decision.signal = Some(ChordSignal::Pressed);
            if self.chord.uses_win() {
                self.mask_win = true;
            }
            if bit.is_none() {
                decision.swallow = true;
                self.swallow_key_up = true;
            }
        }
        decision
    }

    pub fn key_up(&mut self, vk_code: u32) -> HookDecision {
        let mut decision = HookDecision::default();
        let bit = modifier_bit(vk_code);
        match bit {
            Some(bit) => self.held &= !bit,
            None if self.chord.key == Some(vk_code) => self.key_held = false,
            None => {}
        }

        if self.active && self.chord.is_part(vk_code) {
            self.active = false;
            decision.signal = Some(ChordSignal::Released);
        }
        if bit.is_none() && self.chord.key == Some(vk_code) && self.swallow_key_up {
            self.swallow_key_up = false;
            decision.swallow = true;
        }
        if bit == Some(MOD_WIN) && self.mask_win {
            self.mask_win = false;
            decision.swallow = true;
            decision.inject_win_release = Some(vk_code);
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker(text: &str) -> ChordTracker {
        ChordTracker::new(Chord::parse(text).unwrap())
    }

    #[test]
    fn parses_shortcuts() {
        assert_eq!(Chord::parse("Ctrl+Win").unwrap().label(), "Ctrl+Win");
        assert_eq!(Chord::parse("win + ctrl").unwrap().label(), "Ctrl+Win");
        assert_eq!(Chord::parse("Ctrl+Alt+Space").unwrap().label(), "Ctrl+Alt+Space");
        assert_eq!(Chord::parse("F9").unwrap().label(), "F9");
        assert_eq!(Chord::parse("ctrl+shift+d").unwrap().label(), "Ctrl+Shift+D");
        assert!(Chord::parse("").is_err());
        assert!(Chord::parse("Ctrl").is_err());
        assert!(Chord::parse("Ctrl+A+B").is_err());
        assert!(Chord::parse("Ctrl+Hund").is_err());
    }

    #[test]
    fn ctrl_then_win_press_and_release() {
        let mut t = tracker("Ctrl+Win");
        assert_eq!(t.key_down(vk::LCONTROL).signal, None);
        let d = t.key_down(vk::LWIN);
        assert_eq!(d.signal, Some(ChordSignal::Pressed));
        assert!(!d.swallow);
        // Auto-repeat of Win while held does nothing.
        assert_eq!(t.key_down(vk::LWIN), HookDecision::default());
        let up = t.key_up(vk::LWIN);
        assert_eq!(up.signal, Some(ChordSignal::Released));
        assert!(up.swallow);
        assert_eq!(up.inject_win_release, Some(vk::LWIN));
        assert_eq!(t.key_up(vk::LCONTROL), HookDecision::default());
    }

    #[test]
    fn win_then_ctrl_release_ctrl_first() {
        let mut t = tracker("Ctrl+Win");
        t.key_down(vk::RWIN);
        assert_eq!(t.key_down(vk::RCONTROL).signal, Some(ChordSignal::Pressed));
        assert_eq!(t.key_up(vk::RCONTROL).signal, Some(ChordSignal::Released));
        // Win key-up is still masked even though the chord ended on Ctrl.
        let up = t.key_up(vk::RWIN);
        assert!(up.swallow);
        assert_eq!(up.inject_win_release, Some(vk::RWIN));
        assert_eq!(up.signal, None);
    }

    #[test]
    fn plain_win_press_is_untouched() {
        let mut t = tracker("Ctrl+Win");
        assert_eq!(t.key_down(vk::LWIN), HookDecision::default());
        assert_eq!(t.key_up(vk::LWIN), HookDecision::default());
    }

    #[test]
    fn other_key_interrupts() {
        let mut t = tracker("Ctrl+Win");
        t.key_down(vk::LCONTROL);
        t.key_down(vk::LWIN);
        let d = t.key_down('D' as u32);
        assert_eq!(d.signal, Some(ChordSignal::Interrupted));
        assert!(!d.swallow);
        assert_eq!(t.key_up('D' as u32).signal, None);
        assert_eq!(t.key_up(vk::LWIN).signal, None);
        assert_eq!(t.key_up(vk::LCONTROL).signal, None);
    }

    #[test]
    fn extra_modifier_interrupts_and_blocks() {
        let mut t = tracker("Ctrl+Win");
        t.key_down(vk::LCONTROL);
        t.key_down(vk::LWIN);
        assert_eq!(t.key_down(vk::LMENU).signal, Some(ChordSignal::Interrupted));
        t.key_up(vk::LMENU);
        // Shortcut is not re-armed until it is pressed again from scratch.
        assert!(!t.is_active());

        let mut t = tracker("Ctrl+Win");
        t.key_down(vk::LSHIFT);
        t.key_down(vk::LCONTROL);
        assert_eq!(t.key_down(vk::LWIN).signal, None);
    }

    #[test]
    fn key_chord_swallows_key() {
        let mut t = tracker("Ctrl+Alt+Space");
        t.key_down(vk::LCONTROL);
        t.key_down(vk::LMENU);
        let d = t.key_down(vk::SPACE);
        assert_eq!(d.signal, Some(ChordSignal::Pressed));
        assert!(d.swallow);
        assert!(t.key_down(vk::SPACE).swallow);
        let up = t.key_up(vk::SPACE);
        assert_eq!(up.signal, Some(ChordSignal::Released));
        assert!(up.swallow);
        assert_eq!(up.inject_win_release, None);
    }

    #[test]
    fn missed_win_release_does_not_stick() {
        let mut t = tracker("Ctrl+Win");
        // Win goes down, then Win+L locks the screen and the key-up is never seen.
        t.key_down(vk::LWIN);
        // After unlocking, only Ctrl is physically down.
        assert_eq!(t.resync(|vk| vk == vk::LCONTROL || vk == vk::CONTROL), None);
        let d = t.key_down(vk::LCONTROL);
        assert_eq!(d.signal, None);
        assert!(!t.is_active());
    }

    #[test]
    fn resync_keeps_held_modifiers() {
        let mut t = tracker("Ctrl+Win");
        t.key_down(vk::LWIN);
        assert_eq!(t.resync(|vk| vk == vk::LWIN), None);
        assert_eq!(t.key_down(vk::LCONTROL).signal, Some(ChordSignal::Pressed));
        assert_eq!(t.resync(|vk| matches!(vk, vk::LWIN | vk::CONTROL)), None);
        assert!(t.is_active());
    }

    #[test]
    fn resync_interrupts_active_chord_on_lost_modifier() {
        let mut t = tracker("Ctrl+Win");
        t.key_down(vk::LCONTROL);
        t.key_down(vk::LWIN);
        assert!(t.is_active());
        assert_eq!(t.resync(|vk| vk == vk::CONTROL), Some(ChordSignal::Interrupted));
        assert!(!t.is_active());
    }

    #[test]
    fn exposes_modifiers_and_key() {
        let chord = Chord::parse("Ctrl+Alt+Space").unwrap();
        assert_eq!(
            chord.modifiers(),
            Modifiers {
                ctrl: true,
                alt: true,
                ..Modifiers::default()
            }
        );
        assert_eq!(chord.key(), Some(vk::SPACE));
        let chord = Chord::parse("Ctrl+Win").unwrap();
        assert!(chord.modifiers().win && chord.modifiers().ctrl);
        assert_eq!(chord.key(), None);
    }

    #[test]
    fn function_key_alone() {
        let mut t = tracker("F9");
        let d = t.key_down(vk::F1 + 8);
        assert_eq!(d.signal, Some(ChordSignal::Pressed));
        assert_eq!(t.key_up(vk::F1 + 8).signal, Some(ChordSignal::Released));
        // With Ctrl held F9 is a different shortcut.
        t.key_down(vk::LCONTROL);
        assert_eq!(t.key_down(vk::F1 + 8).signal, None);
    }
}
