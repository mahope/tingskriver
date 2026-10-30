//! Push-to-talk gesture: hold to talk, release to insert, double tap to lock.
//!
//! Recording starts on the first press so no speech is lost. A short tap is
//! only a candidate for a double tap; if no second press follows within the
//! window, the recording is cancelled.

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

#[derive(Clone, Copy, Debug)]
pub struct GestureConfig {
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
            tap_max_ms: 300,
            double_tap_window_ms: 400,
            max_recording_ms: 10 * 60 * 1000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Holding { since: u64 },
    TapPending { released_at: u64 },
    Locked,
    /// Recording is done; wait for the shortcut to be released.
    AwaitRelease,
}

#[derive(Debug)]
pub struct Gesture {
    config: GestureConfig,
    state: State,
    recording_since: u64,
}

impl Gesture {
    pub fn new(config: GestureConfig) -> Self {
        Gesture {
            config,
            state: State::Idle,
            recording_since: 0,
        }
    }

    pub fn is_recording(&self) -> bool {
        matches!(
            self.state,
            State::Holding { .. } | State::TapPending { .. } | State::Locked
        )
    }

    pub fn is_locked(&self) -> bool {
        self.state == State::Locked
    }

    /// Abort from outside (e.g. the user pressed Escape in the overlay or an error).
    pub fn reset(&mut self) {
        self.state = match self.state {
            State::Holding { .. } => State::AwaitRelease,
            _ => State::Idle,
        };
    }

    pub fn on_event(&mut self, event: GestureEvent, now: u64) -> Vec<GestureAction> {
        use GestureAction::*;
        use GestureEvent::*;

        let mut actions = self.tick(now);
        match (self.state, event) {
            (State::Idle, Pressed) => {
                self.state = State::Holding { since: now };
                self.recording_since = now;
                actions.push(Start);
            }
            (State::Holding { since }, Released) => {
                if now.saturating_sub(since) < self.config.tap_max_ms {
                    self.state = State::TapPending { released_at: now };
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
        match self.state {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GestureAction::*;
    use super::GestureEvent::*;
    use super::*;

    fn gesture() -> Gesture {
        Gesture::new(GestureConfig {
            tap_max_ms: 300,
            double_tap_window_ms: 400,
            max_recording_ms: 60_000,
        })
    }

    #[test]
    fn hold_and_release_inserts() {
        let mut g = gesture();
        assert_eq!(g.on_event(Pressed, 1000), vec![Start]);
        assert!(g.is_recording());
        assert_eq!(g.tick(2000), vec![]);
        assert_eq!(g.on_event(Released, 3000), vec![Stop]);
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
