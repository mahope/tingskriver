//! What the pill shows and when, without any window code.
//!
//! [`Pill`] is a small state machine driven by the caller's clock: the
//! window layer feeds it state changes and frame ticks and asks whether the
//! pill is visible and whether it still needs frames. That keeps timing
//! (auto-hide, fade-out, bar history) testable on any OS.

use std::time::{Duration, Instant};

use crate::theme::{self, size, timing, Hsl};

/// What the pill shows.
///
/// The names in the issue (Danish) map one to one: `Inaktiv` = [`Idle`],
/// `Lytter{niveau}` = [`Listening`], `Behandler` = [`Processing`],
/// `Færdig` = [`Done`], `Fejl(tekst)` = [`Error`], `Afbrudt` = [`Cancelled`].
///
/// [`Idle`]: PillState::Idle
/// [`Listening`]: PillState::Listening
/// [`Processing`]: PillState::Processing
/// [`Done`]: PillState::Done
/// [`Error`]: PillState::Error
/// [`Cancelled`]: PillState::Cancelled
#[derive(Clone, Debug, PartialEq)]
pub enum PillState {
    /// Nothing to show; the pill is hidden.
    Idle,
    /// Recording. `level` is the input level, 0.0..=1.0; `hands_free` when
    /// dictation is locked on and the shortcut can be released.
    Listening { level: f32, hands_free: bool },
    /// Transcribing and cleaning up ("Skriver …").
    Processing,
    /// Finished; hides itself after a moment.
    Done,
    /// Something failed; the text is shown and the pill hides itself.
    Error(String),
    /// Stopped without result; hides itself after a moment.
    Cancelled,
}

/// The small mark left of the label.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Indicator {
    Dot(Hsl),
    Spinner,
    Check,
}

/// Everything the renderer needs to know about a state.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub indicator: Indicator,
    pub bars: bool,
    pub label: String,
}

impl PillState {
    /// How the state looks, or `None` when nothing is shown.
    pub fn look(&self) -> Option<Look> {
        let look = |indicator, bars, label: &str| {
            Some(Look {
                indicator,
                bars,
                label: label.to_string(),
            })
        };
        match self {
            PillState::Idle => None,
            PillState::Listening {
                hands_free: false, ..
            } => look(Indicator::Dot(theme::RECORDING), true, "Lytter"),
            PillState::Listening {
                hands_free: true, ..
            } => look(Indicator::Dot(theme::PRIMARY), true, "Håndfri"),
            PillState::Processing => look(Indicator::Spinner, false, "Skriver …"),
            PillState::Done => look(Indicator::Check, false, "Færdig"),
            PillState::Error(text) => {
                let text = text.trim();
                let text = if text.is_empty() {
                    "Kunne ikke skrive"
                } else {
                    text
                };
                look(Indicator::Dot(theme::DESTRUCTIVE), false, text)
            }
            PillState::Cancelled => look(Indicator::Dot(theme::MUTED_FOREGROUND), false, "Afbrudt"),
        }
    }

    /// States that end by themselves hide after this long.
    pub fn auto_hide_after(&self) -> Option<Duration> {
        match self {
            PillState::Done => Some(Duration::from_millis(timing::DONE_HIDE_MS)),
            PillState::Cancelled => Some(Duration::from_millis(timing::CANCELLED_HIDE_MS)),
            PillState::Error(_) => Some(Duration::from_millis(timing::ERROR_HIDE_MS)),
            _ => None,
        }
    }

    fn level(&self) -> Option<f32> {
        match self {
            PillState::Listening { level, .. } => Some(level.clamp(0.0, 1.0)),
            _ => None,
        }
    }

    /// Same variant (and same text) as `other`; the level does not count.
    fn same_kind(&self, other: &PillState) -> bool {
        match (self, other) {
            (
                PillState::Listening { hands_free: a, .. },
                PillState::Listening { hands_free: b, .. },
            ) => a == b,
            (PillState::Error(a), PillState::Error(b)) => a == b,
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Hidden,
    Shown,
    FadingOut { since: Instant },
}

/// What the window layer has to do after a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Nothing visible changed.
    None,
    /// The pill was hidden and must be placed and shown now.
    Show,
    /// The pill is visible and must be redrawn.
    Redraw,
}

/// One frame as the window layer draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub look: Look,
    /// Bar heights in logical pixels, oldest first.
    pub bars: [f32; size::BAR_COUNT],
    /// Spinner angle in turns, 0.0..1.0.
    pub spinner: f32,
    /// Window opacity, 0.0..=1.0 (fade-out).
    pub opacity: f32,
}

/// The pill's state machine. All methods take the current time, so tests
/// can run it on a fake clock.
#[derive(Debug)]
pub struct Pill {
    state: PillState,
    /// Shown state while fading out, so the fade draws the old content.
    fading: Option<PillState>,
    phase: Phase,
    enabled: bool,
    state_since: Instant,
    level: f32,
    shown_level: f32,
    history: [f32; size::BAR_COUNT],
    last_step: Instant,
    reduced_motion: bool,
}

impl Pill {
    pub fn new(now: Instant) -> Self {
        Self {
            state: PillState::Idle,
            fading: None,
            phase: Phase::Hidden,
            enabled: true,
            state_since: now,
            level: 0.0,
            shown_level: 0.0,
            history: [0.0; size::BAR_COUNT],
            last_step: now,
            reduced_motion: false,
        }
    }

    /// Turn animations off (Windows' "Vis animationer" setting).
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
    }

    pub fn state(&self) -> &PillState {
        &self.state
    }

    /// True while the window must be on screen (also while fading out).
    pub fn is_visible(&self) -> bool {
        self.phase != Phase::Hidden
    }

    /// Frames are needed exactly while the pill is visible.
    pub fn needs_frames(&self) -> bool {
        self.is_visible()
    }

    /// Change what the pill shows. Any state but [`PillState::Idle`] also
    /// shows the pill; `Idle` hides it.
    pub fn set_state(&mut self, mut state: PillState, now: Instant) -> Change {
        if let PillState::Listening { level, .. } = &mut state {
            *level = level.clamp(0.0, 1.0);
        }
        if let Some(level) = state.level() {
            self.level = level;
        }
        if state.same_kind(&self.state) {
            // Only the level moved; the next frame picks it up.
            self.state = state;
            return Change::None;
        }
        let entering_listening = state.level().is_some() && self.state.level().is_none();
        let previous = std::mem::replace(&mut self.state, state);
        self.state_since = now;
        if entering_listening {
            self.history = [0.0; size::BAR_COUNT];
            self.shown_level = 0.0;
            self.last_step = now;
        }
        self.enabled = self.state != PillState::Idle;
        self.apply_visibility(now, previous)
    }

    /// Update only the input level (cheap, no state change).
    pub fn set_level(&mut self, level: f32) {
        self.level = level.clamp(0.0, 1.0);
        if let PillState::Listening { level: l, .. } = &mut self.state {
            *l = self.level;
        }
    }

    /// Show the current state again after [`Pill::hide`].
    pub fn show(&mut self, now: Instant) -> Change {
        self.enabled = true;
        self.apply_visibility(now, self.state.clone())
    }

    /// Hide the pill but keep its state.
    pub fn hide(&mut self, now: Instant) -> Change {
        self.enabled = false;
        self.apply_visibility(now, self.state.clone())
    }

    fn should_show(&self) -> bool {
        self.enabled && self.state.look().is_some()
    }

    /// `shown` is what the pill showed before this change, drawn while it
    /// fades out.
    fn apply_visibility(&mut self, now: Instant, shown: PillState) -> Change {
        match (self.should_show(), self.phase) {
            (true, Phase::Hidden) => {
                self.fading = None;
                self.phase = Phase::Shown;
                Change::Show
            }
            (true, Phase::FadingOut { .. }) => {
                self.fading = None;
                self.phase = Phase::Shown;
                Change::Redraw
            }
            (true, Phase::Shown) => Change::Redraw,
            (false, Phase::Shown) => {
                if self.reduced_motion {
                    self.phase = Phase::Hidden;
                    self.fading = None;
                } else {
                    self.phase = Phase::FadingOut { since: now };
                    self.fading = Some(shown);
                }
                Change::Redraw
            }
            (false, _) => Change::None,
        }
    }

    /// Advance timers: auto-hide, fade-out and the bar history. Call it for
    /// every frame while [`Pill::needs_frames`] is true.
    pub fn tick(&mut self, now: Instant) {
        if self.phase == Phase::Shown {
            if let Some(after) = self.state.auto_hide_after() {
                if now.saturating_duration_since(self.state_since) >= after {
                    // Keep the content for the fade, then go idle.
                    self.fading = Some(self.state.clone());
                    self.state = PillState::Idle;
                    self.state_since = now;
                    self.enabled = false;
                    self.phase = if self.reduced_motion {
                        Phase::Hidden
                    } else {
                        Phase::FadingOut { since: now }
                    };
                }
            }
        }
        if let Phase::FadingOut { since } = self.phase {
            if now.saturating_duration_since(since) >= Duration::from_millis(timing::FADE_OUT_MS) {
                self.phase = Phase::Hidden;
                self.fading = None;
            }
        }
        self.step_bars(now);
    }

    fn step_bars(&mut self, now: Instant) {
        let step = Duration::from_millis(timing::BAR_STEP_MS);
        // A level jump shows at once; a drop decays so the bars do not flicker.
        self.shown_level = if self.level >= self.shown_level {
            self.level
        } else {
            (self.shown_level * 0.7).max(self.level)
        };
        let mut steps = 0;
        while now.saturating_duration_since(self.last_step) >= step && steps < size::BAR_COUNT {
            self.history.rotate_left(1);
            self.history[size::BAR_COUNT - 1] = self.shown_level;
            self.last_step += step;
            steps += 1;
        }
        if now.saturating_duration_since(self.last_step) >= step {
            // Long gap (the thread was busy): catch up in one go.
            self.last_step = now;
        }
    }

    /// The frame to draw now, or `None` when hidden.
    pub fn frame(&self, now: Instant) -> Option<Frame> {
        let opacity = match self.phase {
            Phase::Hidden => return None,
            Phase::Shown => 1.0,
            Phase::FadingOut { since } => {
                let t = now.saturating_duration_since(since).as_secs_f32()
                    / (timing::FADE_OUT_MS as f32 / 1000.0);
                (1.0 - t).clamp(0.0, 1.0)
            }
        };
        let state = self.fading.as_ref().unwrap_or(&self.state);
        let look = state.look()?;
        let spinner = if self.reduced_motion {
            0.0
        } else {
            let turn = timing::SPINNER_TURN_MS as f32 / 1000.0;
            (now.saturating_duration_since(self.state_since)
                .as_secs_f32()
                / turn)
                .fract()
        };
        Some(Frame {
            bars: bar_heights(&self.history),
            look,
            spinner,
            opacity,
        })
    }
}

/// Bar heights for a level history: middle bars a little taller, so it
/// reads as a voice meter (same shape as the webview pill).
pub fn bar_heights(history: &[f32; size::BAR_COUNT]) -> [f32; size::BAR_COUNT] {
    let n = size::BAR_COUNT as f32;
    let mid = (n - 1.0) / 2.0;
    let mut out = [size::BAR_MIN; size::BAR_COUNT];
    for (i, level) in history.iter().enumerate() {
        let shape = 1.0 - (i as f32 - mid).abs() / n;
        let h = size::BAR_MAX * level.clamp(0.0, 1.0) * (0.6 + shape * 0.6);
        out[i] = h.clamp(size::BAR_MIN, size::BAR_MAX);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn listening(level: f32) -> PillState {
        PillState::Listening {
            level,
            hands_free: false,
        }
    }

    #[test]
    fn labels_are_danish() {
        let label = |s: PillState| s.look().unwrap().label;
        assert_eq!(label(listening(0.0)), "Lytter");
        assert_eq!(
            label(PillState::Listening {
                level: 0.0,
                hands_free: true
            }),
            "Håndfri"
        );
        assert_eq!(label(PillState::Processing), "Skriver …");
        assert_eq!(label(PillState::Done), "Færdig");
        assert_eq!(label(PillState::Cancelled), "Afbrudt");
        assert_eq!(
            label(PillState::Error("Ingen mikrofon".into())),
            "Ingen mikrofon"
        );
        assert_eq!(label(PillState::Error("  ".into())), "Kunne ikke skrive");
        assert!(PillState::Idle.look().is_none());
    }

    #[test]
    fn only_listening_shows_bars() {
        assert!(listening(0.5).look().unwrap().bars);
        assert!(!PillState::Processing.look().unwrap().bars);
        assert_eq!(
            PillState::Processing.look().unwrap().indicator,
            Indicator::Spinner
        );
        assert_eq!(PillState::Done.look().unwrap().indicator, Indicator::Check);
    }

    #[test]
    fn a_state_change_shows_the_pill_at_once() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        assert!(!pill.is_visible());
        assert!(!pill.needs_frames());
        assert_eq!(pill.set_state(listening(0.2), t0), Change::Show);
        assert!(pill.is_visible());
        assert!(pill.frame(t0).is_some());
        assert_eq!(
            pill.set_state(PillState::Processing, t0 + ms(10)),
            Change::Redraw
        );
    }

    #[test]
    fn level_updates_do_not_restart_the_state() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_state(listening(0.1), t0);
        assert_eq!(pill.set_state(listening(0.9), t0 + ms(5)), Change::None);
        assert_eq!(pill.state(), &listening(0.9));
    }

    #[test]
    fn idle_fades_out_and_stops_frames() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_state(listening(0.5), t0);
        assert_eq!(
            pill.set_state(PillState::Idle, t0 + ms(100)),
            Change::Redraw
        );
        // Still drawing the old content while fading.
        let frame = pill.frame(t0 + ms(160)).unwrap();
        assert_eq!(frame.look.label, "Lytter");
        assert!(frame.opacity > 0.0 && frame.opacity < 1.0);
        pill.tick(t0 + ms(100 + timing::FADE_OUT_MS));
        assert!(!pill.is_visible());
        assert!(!pill.needs_frames());
        assert!(pill.frame(t0 + ms(400)).is_none());
    }

    #[test]
    fn reduced_motion_hides_at_once() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_reduced_motion(true);
        pill.set_state(PillState::Processing, t0);
        pill.set_state(PillState::Idle, t0 + ms(10));
        assert!(!pill.is_visible());
    }

    #[test]
    fn finished_states_hide_themselves() {
        for (state, after) in [
            (PillState::Done, timing::DONE_HIDE_MS),
            (PillState::Cancelled, timing::CANCELLED_HIDE_MS),
            (PillState::Error("x".into()), timing::ERROR_HIDE_MS),
        ] {
            let t0 = Instant::now();
            let mut pill = Pill::new(t0);
            pill.set_state(state.clone(), t0);
            pill.tick(t0 + ms(after - 1));
            assert_eq!(pill.state(), &state);
            pill.tick(t0 + ms(after));
            assert_eq!(pill.state(), &PillState::Idle);
            assert!(pill.is_visible(), "fades out first");
            pill.tick(t0 + ms(after + timing::FADE_OUT_MS));
            assert!(!pill.is_visible());
        }
    }

    #[test]
    fn listening_does_not_hide_itself() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_state(listening(0.3), t0);
        pill.tick(t0 + Duration::from_secs(600));
        assert!(pill.is_visible());
    }

    #[test]
    fn showing_again_during_fade_cancels_it() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_state(PillState::Processing, t0);
        pill.set_state(PillState::Idle, t0 + ms(10));
        assert_eq!(pill.set_state(listening(0.1), t0 + ms(20)), Change::Redraw);
        pill.tick(t0 + ms(500));
        assert!(pill.is_visible());
        assert_eq!(pill.frame(t0 + ms(500)).unwrap().opacity, 1.0);
    }

    #[test]
    fn hide_and_show_keep_the_state() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_reduced_motion(true);
        pill.set_state(PillState::Processing, t0);
        pill.hide(t0);
        assert!(!pill.is_visible());
        assert_eq!(pill.state(), &PillState::Processing);
        assert_eq!(pill.show(t0), Change::Show);
        // Nothing to show while idle.
        pill.set_state(PillState::Idle, t0);
        assert_eq!(pill.show(t0), Change::None);
        assert!(!pill.is_visible());
    }

    #[test]
    fn bars_follow_the_level_over_time() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_state(listening(0.0), t0);
        let quiet = pill.frame(t0).unwrap().bars;
        assert!(quiet.iter().all(|&h| h == size::BAR_MIN));
        pill.set_level(1.0);
        let mut t = t0;
        for _ in 0..size::BAR_COUNT {
            t += ms(timing::BAR_STEP_MS);
            pill.tick(t);
        }
        let loud = pill.frame(t).unwrap().bars;
        assert!(loud.iter().all(|&h| h > size::BAR_MIN));
        // The middle bar is the tallest.
        let mid = size::BAR_COUNT / 2;
        assert!(loud.iter().all(|&h| h <= loud[mid]));
        assert!(loud[mid] <= size::BAR_MAX);
    }

    #[test]
    fn bars_reset_when_listening_starts_again() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_state(listening(1.0), t0);
        pill.tick(t0 + ms(400));
        pill.set_state(PillState::Processing, t0 + ms(400));
        pill.set_state(listening(0.0), t0 + ms(500));
        let bars = pill.frame(t0 + ms(500)).unwrap().bars;
        assert!(bars.iter().all(|&h| h == size::BAR_MIN));
    }

    #[test]
    fn spinner_turns_and_stands_still_with_reduced_motion() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_state(PillState::Processing, t0);
        let a = pill.frame(t0 + ms(100)).unwrap().spinner;
        let b = pill.frame(t0 + ms(300)).unwrap().spinner;
        assert!(a != b && (0.0..1.0).contains(&a) && (0.0..1.0).contains(&b));
        pill.set_reduced_motion(true);
        assert_eq!(pill.frame(t0 + ms(300)).unwrap().spinner, 0.0);
    }

    #[test]
    fn levels_are_clamped() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_state(listening(7.0), t0);
        assert_eq!(pill.state(), &listening(1.0));
        let heights = bar_heights(&[2.0; size::BAR_COUNT]);
        assert!(heights.iter().all(|&h| h <= size::BAR_MAX));
    }
}
