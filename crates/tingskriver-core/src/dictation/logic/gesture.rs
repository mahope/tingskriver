//! Configurable dictation shortcut gestures with debounced release.
//!
//! Recording starts on the first press so no speech is lost. Double-tap mode
//! cancels an unmatched short tap; hybrid mode keeps it recording hands-free.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureEvent {
    Pressed,
    Released,
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureAction {
    /// Start recording.
    Start,
    /// Hands-free: keep recording until the next press.
    Lock,
    /// Stop recording and insert the text.
    Stop,
    /// Stop recording and throw the audio away.
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShortcutMode {
    Hold,
    Toggle,
    Hybrid,
    #[default]
    DoubleTap,
}

#[derive(Clone, Copy, Debug)]
pub struct GestureConfig {
    pub mode: ShortcutMode,
    /// A press shorter than this counts as a tap.
    pub tap_max_ms: u64,
    /// Second press must come within this long after the first tap's release.
    pub double_tap_window_ms: u64,
    /// Recording is stopped automatically after this long.
    pub max_recording_ms: u64,
}

impl Default for GestureConfig {
    fn default() -> Self {
        GestureConfig {
            mode: ShortcutMode::DoubleTap,
            tap_max_ms: 300,
            double_tap_window_ms: 400,
            max_recording_ms: 10 * 60 * 1000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Holding {
        since: u64,
    },
    TapPending {
        released_at: u64,
    },
    Locked,
    /// Recording is done; wait for the shortcut to be released.
    AwaitRelease,
}

#[derive(Debug)]
pub struct Gesture {
    config: GestureConfig,
    state: State,
    recording_since: u64,
    down: bool,
    release_at: Option<u64>,
}

impl Gesture {
    pub fn new(config: GestureConfig) -> Self {
        Gesture {
            config,
            state: State::Idle,
            recording_since: 0,
            down: false,
            release_at: None,
        }
    }

    pub fn is_recording(&self) -> bool {
        matches!(
            self.state,
            State::Holding { .. } | State::TapPending { .. } | State::Locked
        )
    }

    pub fn set_mode(&mut self, mode: ShortcutMode) {
        if self.state == State::Idle && !self.down {
            self.config.mode = mode;
        }
    }

    pub fn set_tap_max_ms(&mut self, ms: u64) {
        if self.state == State::Idle && !self.down {
            self.config.tap_max_ms = ms.clamp(100, 1000);
        }
    }

    pub fn is_locked(&self) -> bool {
        self.state == State::Locked
    }

    /// Abort from outside (e.g. the user pressed Escape in the overlay or an error).
    pub fn reset(&mut self) {
        self.release_at = None;
        self.state = match self.down {
            true => State::AwaitRelease,
            false => State::Idle,
        };
    }

    pub fn on_event(&mut self, event: GestureEvent, now: u64) -> Vec<GestureAction> {
        use GestureAction::*;
        use GestureEvent::*;

        if event == Interrupted {
            let recording = self.is_recording();
            // The tracker deactivates on interruption and sends no release.
            self.down = false;
            self.reset();
            return if recording { vec![Cancel] } else { vec![] };
        }
        if event == Pressed
            && self
                .release_at
                .is_some_and(|at| now.saturating_sub(at) < 30)
        {
            self.release_at = None;
            self.down = true;
            return vec![];
        }
        let mut actions = self.tick(now);
        if event == Pressed {
            if let Some(at) = self.release_at.take() {
                actions.extend(self.transition(Released, at));
            }
        }
        if event == Released {
            if self.down {
                self.down = false;
                self.release_at = Some(now);
            }
            return actions;
        }
        if self.down {
            return actions;
        }
        self.down = true;
        actions.extend(self.transition(event, now));
        actions
    }

    fn transition(&mut self, event: GestureEvent, now: u64) -> Vec<GestureAction> {
        use GestureAction::*;
        use GestureEvent::*;
        let mut actions = Vec::new();
        match (self.state, event) {
            (State::Idle, Pressed) => {
                self.state = if self.config.mode == ShortcutMode::Toggle {
                    State::Locked
                } else {
                    State::Holding { since: now }
                };
                self.recording_since = now;
                actions.push(Start);
            }
            (State::Holding { since }, Released) => {
                if self.config.mode != ShortcutMode::Hold
                    && now.saturating_sub(since) < self.config.tap_max_ms
                {
                    self.state = if self.config.mode == ShortcutMode::Hybrid {
                        actions.push(Lock);
                        State::Locked
                    } else {
                        State::TapPending { released_at: now }
                    };
                } else {
                    self.state = State::Idle;
                    actions.push(Stop);
                }
            }
            (State::Holding { .. }, Interrupted) => {
                self.state = State::Idle;
                actions.push(Cancel);
            }
            (State::TapPending { .. }, Pressed) => {
                self.state = State::Locked;
                actions.push(Lock);
            }
            (State::Locked, Pressed) => {
                self.state = State::AwaitRelease;
                actions.push(Stop);
            }
            (State::Locked, Interrupted) => {
                self.state = State::Idle;
                actions.push(Cancel);
            }
            (State::AwaitRelease, Released | Interrupted) => {
                self.state = State::Idle;
            }
            _ => {}
        }
        actions
    }

    /// Call regularly (every ~50 ms) to expire taps and over-long recordings.
    pub fn tick(&mut self, now: u64) -> Vec<GestureAction> {
        let mut actions = Vec::new();
        if let Some(at) = self.release_at.filter(|at| now.saturating_sub(*at) >= 50) {
            self.release_at = None;
            actions.extend(self.transition(GestureEvent::Released, at));
        }
        actions.extend(match self.state {
            State::TapPending { released_at }
                if now.saturating_sub(released_at) > self.config.double_tap_window_ms =>
            {
                self.state = State::Idle;
                vec![GestureAction::Cancel]
            }
            State::Holding { .. } | State::Locked | State::TapPending { .. }
                if now.saturating_sub(self.recording_since) >= self.config.max_recording_ms =>
            {
                self.state = match self.state {
                    State::Holding { .. } => State::AwaitRelease,
                    _ => State::Idle,
                };
                vec![GestureAction::Stop]
            }
            _ => Vec::new(),
        });
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::GestureAction::*;
    use super::GestureEvent::*;
    use super::*;

    fn gesture() -> Gesture {
        Gesture::new(GestureConfig {
            max_recording_ms: 60_000,
            ..GestureConfig::default()
        })
    }

    fn with_mode(mode: ShortcutMode) -> Gesture {
        Gesture::new(GestureConfig {
            mode,
            ..GestureConfig::default()
        })
    }

    #[test]
    fn hybrid_uses_configured_tap_threshold() {
        for (duration, expected) in [(599, GestureAction::Lock), (600, GestureAction::Stop)] {
            let mut gesture = Gesture::new(GestureConfig {
                mode: ShortcutMode::Hybrid,
                ..GestureConfig::default()
            });
            gesture.set_tap_max_ms(600);
            assert_eq!(
                gesture.on_event(GestureEvent::Pressed, 0),
                vec![GestureAction::Start]
            );
            gesture.on_event(GestureEvent::Released, duration);
            assert_eq!(gesture.tick(duration + 50), vec![expected]);
        }
    }

    #[test]
    fn persisted_modes_round_trip() {
        for (mode, name) in [
            (ShortcutMode::Hold, "hold"),
            (ShortcutMode::Toggle, "toggle"),
            (ShortcutMode::Hybrid, "hybrid"),
            (ShortcutMode::DoubleTap, "double-tap"),
        ] {
            let json = format!("\"{name}\"");
            assert_eq!(serde_json::to_string(&mode).unwrap(), json);
            assert_eq!(serde_json::from_str::<ShortcutMode>(&json).unwrap(), mode);
        }
        assert_eq!(ShortcutMode::default(), ShortcutMode::DoubleTap);
        assert!(serde_json::from_str::<ShortcutMode>("\"unknown\"").is_err());
    }

    #[test]
    fn hold_short_tap_stops_only_after_release_grace() {
        let mut g = with_mode(ShortcutMode::Hold);
        assert_eq!(g.on_event(Pressed, 0), vec![Start]);
        assert_eq!(g.on_event(Released, 40), vec![]);
        assert_eq!(g.tick(89), vec![]);
        assert_eq!(g.tick(90), vec![Stop]);
    }

    #[test]
    fn toggle_ignores_release_and_key_repeat() {
        let mut g = with_mode(ShortcutMode::Toggle);
        assert_eq!(g.on_event(Pressed, 0), vec![Start]);
        assert_eq!(g.on_event(Pressed, 40), vec![]);
        assert_eq!(g.on_event(Released, 100), vec![]);
        assert_eq!(g.tick(150), vec![]);
        assert_eq!(g.on_event(Pressed, 200), vec![Stop]);
        assert_eq!(g.on_event(Pressed, 240), vec![]);
    }

    #[test]
    fn hybrid_boundary_is_300_ms() {
        for (release, expected) in [(299, Lock), (300, Stop)] {
            let mut g = with_mode(ShortcutMode::Hybrid);
            assert_eq!(g.on_event(Pressed, 0), vec![Start]);
            g.on_event(Released, release);
            assert_eq!(g.tick(release + 50), vec![expected]);
            if release == 299 {
                assert_eq!(g.on_event(Pressed, 400), vec![Stop]);
            }
        }
    }

    #[test]
    fn release_bounce_under_30_ms_does_not_stop_or_lock() {
        for mode in [
            ShortcutMode::Hold,
            ShortcutMode::Toggle,
            ShortcutMode::Hybrid,
            ShortcutMode::DoubleTap,
        ] {
            let mut g = with_mode(mode);
            assert_eq!(g.on_event(Pressed, 0), vec![Start]);
            g.on_event(Released, 400);
            assert_eq!(g.on_event(Pressed, 429), vec![]);
            assert_eq!(g.tick(500), vec![]);
            assert!(g.is_recording());
        }
    }

    #[test]
    fn a_second_press_at_debounce_boundary_is_a_double_tap() {
        let mut g = with_mode(ShortcutMode::DoubleTap);
        g.on_event(Pressed, 0);
        g.on_event(Released, 100);
        assert_eq!(g.on_event(Pressed, 130), vec![Lock]);
        assert!(g.is_recording());
        assert_eq!(g.tick(150), vec![]);
    }

    #[test]
    fn reset_does_not_restart_from_key_repeat() {
        let mut g = with_mode(ShortcutMode::Toggle);
        g.on_event(Pressed, 0);
        g.reset();
        assert_eq!(g.on_event(Pressed, 100), vec![]);
        g.on_event(Released, 200);
        assert_eq!(g.on_event(Pressed, 300), vec![Start]);
    }

    #[test]
    fn tracker_interruption_allows_next_shortcut_without_release_signal() {
        use crate::dictation::logic::hotkey::{vk, Chord, ChordSignal, ChordTracker};

        for mode in [
            ShortcutMode::Hold,
            ShortcutMode::Toggle,
            ShortcutMode::Hybrid,
            ShortcutMode::DoubleTap,
        ] {
            for resync in [false, true] {
                let mut g = with_mode(mode);
                let mut tracker = ChordTracker::new(Chord::parse("Ctrl+Win").unwrap());
                assert_eq!(tracker.key_down(vk::LCONTROL).signal, None);
                assert_eq!(
                    tracker.key_down(vk::LWIN).signal,
                    Some(ChordSignal::Pressed)
                );
                assert_eq!(g.on_event(Pressed, 0), vec![Start]);
                let signal = if resync {
                    tracker.resync(|_| false)
                } else {
                    tracker.key_down(u32::from('D')).signal
                };
                assert_eq!(signal, Some(ChordSignal::Interrupted));
                assert_eq!(g.on_event(Interrupted, 100), vec![Cancel]);
                assert_eq!(tracker.key_up(u32::from('D')).signal, None);
                assert_eq!(tracker.key_up(vk::LWIN).signal, None);
                assert_eq!(tracker.key_up(vk::LCONTROL).signal, None);
                assert_eq!(g.tick(200), vec![]);
                assert!(!g.is_recording());
                assert_eq!(tracker.key_down(vk::LCONTROL).signal, None);
                assert_eq!(
                    tracker.key_down(vk::LWIN).signal,
                    Some(ChordSignal::Pressed)
                );
                assert_eq!(
                    g.on_event(Pressed, 300),
                    vec![Start],
                    "{mode:?}, resync={resync}"
                );
            }
        }
    }

    #[test]
    fn interruption_cancels_every_recording_mode() {
        for mode in [
            ShortcutMode::Hold,
            ShortcutMode::Toggle,
            ShortcutMode::Hybrid,
            ShortcutMode::DoubleTap,
        ] {
            let mut g = with_mode(mode);
            assert_eq!(g.on_event(Interrupted, 0), vec![]);
            g.on_event(Pressed, 100);
            g.on_event(Released, 200);
            assert_eq!(g.on_event(Interrupted, 220), vec![Cancel]);
            assert_eq!(g.tick(1000), vec![]);
            assert!(!g.is_recording());
        }
    }

    #[test]
    fn hold_and_release_inserts() {
        let mut g = gesture();
        assert_eq!(g.on_event(Pressed, 1000), vec![Start]);
        assert!(g.is_recording());
        assert_eq!(g.tick(2000), vec![]);
        assert_eq!(g.on_event(Released, 3000), vec![]);
        assert_eq!(g.tick(3050), vec![Stop]);
        assert!(!g.is_recording());
    }

    #[test]
    fn single_short_tap_cancels_after_window() {
        let mut g = gesture();
        assert_eq!(g.on_event(Pressed, 1000), vec![Start]);
        assert_eq!(g.on_event(Released, 1100), vec![]);
        assert!(g.is_recording());
        assert_eq!(g.tick(1400), vec![]);
        assert_eq!(g.tick(1501), vec![Cancel]);
        assert!(!g.is_recording());
    }

    #[test]
    fn double_tap_locks_until_next_press() {
        let mut g = gesture();
        g.on_event(Pressed, 1000);
        g.on_event(Released, 1100);
        assert_eq!(g.on_event(Pressed, 1300), vec![Lock]);
        assert!(g.is_locked());
        assert_eq!(g.on_event(Released, 1400), vec![]);
        assert_eq!(g.tick(20_000), vec![]);
        assert!(g.is_recording());
        assert_eq!(g.on_event(Pressed, 30_000), vec![Stop]);
        assert!(!g.is_recording());
        // The release of the stopping press does not start anything.
        assert_eq!(g.on_event(Released, 30_100), vec![]);
        assert_eq!(g.on_event(Pressed, 31_000), vec![Start]);
    }

    #[test]
    fn late_second_press_starts_fresh() {
        let mut g = gesture();
        g.on_event(Pressed, 1000);
        g.on_event(Released, 1100);
        // No tick ran in between: the expired tap is cancelled first.
        assert_eq!(g.on_event(Pressed, 2000), vec![Cancel, Start]);
        assert!(g.is_recording());
        assert!(!g.is_locked());
    }

    #[test]
    fn interrupt_cancels() {
        let mut g = gesture();
        g.on_event(Pressed, 1000);
        assert_eq!(g.on_event(Interrupted, 1500), vec![Cancel]);
        assert!(!g.is_recording());
        assert_eq!(g.on_event(Released, 1600), vec![]);
    }

    #[test]
    fn max_duration_stops_while_holding_and_waits_for_release() {
        let mut g = gesture();
        g.on_event(Pressed, 0);
        assert_eq!(g.tick(60_000), vec![Stop]);
        assert!(!g.is_recording());
        assert_eq!(g.on_event(Released, 61_000), vec![]);
        assert_eq!(g.on_event(Pressed, 62_000), vec![Start]);
    }

    #[test]
    fn max_duration_stops_locked() {
        let mut g = gesture();
        g.on_event(Pressed, 0);
        g.on_event(Released, 100);
        g.on_event(Pressed, 200);
        g.on_event(Released, 300);
        assert_eq!(g.tick(60_000), vec![Stop]);
        assert_eq!(g.on_event(Pressed, 61_000), vec![Start]);
    }

    #[test]
    fn reset_while_holding_waits_for_release() {
        let mut g = gesture();
        g.on_event(Pressed, 0);
        g.reset();
        assert!(!g.is_recording());
        assert_eq!(g.on_event(Released, 500), vec![]);
        assert_eq!(g.on_event(Pressed, 600), vec![Start]);
    }
}
