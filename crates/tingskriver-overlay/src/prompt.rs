//! The prompt window's model: what is asked, which buttons there are, how
//! keys and clicks answer it, and the countdown. Pure and clock-injected, so
//! everything here is tested on any OS. The Win32 window (`windows_prompt`)
//! only shows a [`PromptModel`] and feeds it input.
//!
//! The event API is small on purpose, so the callers (meeting detection #59,
//! calendar reminder #201) need nothing else:
//!
//! ```text
//! let request = PromptRequest::meeting_question(id);   // or reminder / meeting_end
//! let prompt = PromptWindow::new()?;                    // Windows only
//! prompt.show(request);                                 // may be deferred or dropped
//! match prompt.events().recv() { PromptEvent::Answered { id, outcome } => ... }
//! ```

use std::time::{Duration, Instant};

use tingskriver_core::{format_da, strings};

/// Identifies one prompt; the caller picks it and gets it back in the answer.
pub type PromptId = u64;

/// What the prompt is for. Only decides the text and defaults; the window
/// treats all kinds the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    /// "Er du i et møde?" when a meeting is detected (#59).
    MeetingQuestion,
    /// A calendar meeting starts soon (#201).
    MeetingReminder,
    /// "Er mødet slut?" with a countdown.
    MeetingEnd,
}

/// What a button does. The caller decides what each action means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptAction {
    Record,
    Ignore,
    Later,
    StopRecording,
    KeepRecording,
}

/// How a prompt ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptOutcome {
    /// The user chose a button (click, Enter, Space or the shortcut).
    Chosen(PromptAction),
    /// The user closed it with Esc or the dismiss shortcut. No answer.
    Dismissed,
    /// The countdown ran out; this is the action it carries.
    Expired(PromptAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptButton {
    pub action: PromptAction,
    pub label: String,
}

/// An automatic answer after `total`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Countdown {
    pub total: Duration,
    pub on_expire: PromptAction,
    /// Show the remaining time in the text (the end-of-meeting prompt).
    pub visible: bool,
}

/// One prompt to show. The first button is the primary one and the default
/// for Enter and the primary shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptRequest {
    pub id: PromptId,
    pub kind: PromptKind,
    pub title: String,
    pub body: Option<String>,
    pub buttons: Vec<PromptButton>,
    pub countdown: Option<Countdown>,
    /// After this long (from the request) the question is moot and is not
    /// shown, if Focus Assist or a full-screen app held it back.
    pub ttl: Duration,
}

fn button(action: PromptAction, label: &str) -> PromptButton {
    PromptButton {
        action,
        label: label.to_string(),
    }
}

impl PromptRequest {
    /// "Er du i et møde? Optag / Ignorér". Ignored by itself after a minute
    /// on screen, so it never lingers over the user's work.
    pub fn meeting_question(id: PromptId) -> Self {
        Self {
            id,
            kind: PromptKind::MeetingQuestion,
            title: strings::PROMPT_MEETING_TITLE.to_string(),
            body: Some(strings::PROMPT_MEETING_BODY.to_string()),
            buttons: vec![
                button(PromptAction::Record, strings::PROMPT_RECORD),
                button(PromptAction::Ignore, strings::PROMPT_IGNORE),
            ],
            countdown: Some(Countdown {
                total: Duration::from_secs(60),
                on_expire: PromptAction::Ignore,
                visible: false,
            }),
            ttl: Duration::from_secs(120),
        }
    }

    /// A meeting starts in `minutes_before` minutes: "Optag mødet / Ikke nu".
    /// Not out of date until the meeting has started.
    pub fn meeting_reminder(id: PromptId, minutes_before: u32) -> Self {
        let seconds = u64::from(minutes_before) * 60;
        Self {
            id,
            kind: PromptKind::MeetingReminder,
            title: strings::prompt_reminder_title(&format_da::duration(seconds)),
            body: None,
            buttons: vec![
                button(PromptAction::Record, strings::PROMPT_REMINDER_RECORD),
                button(PromptAction::Later, strings::PROMPT_REMINDER_LATER),
            ],
            countdown: Some(Countdown {
                total: Duration::from_secs(30),
                on_expire: PromptAction::Later,
                visible: false,
            }),
            ttl: Duration::from_secs(seconds),
        }
    }

    /// "Er mødet slut?" with a visible countdown. When it runs out the
    /// recording stops, which is what an unattended, finished meeting needs;
    /// a caller that wants another default builds the request itself.
    pub fn meeting_end(id: PromptId, countdown: Duration) -> Self {
        Self {
            id,
            kind: PromptKind::MeetingEnd,
            title: strings::PROMPT_END_TITLE.to_string(),
            body: None,
            buttons: vec![
                button(PromptAction::KeepRecording, strings::PROMPT_END_KEEP),
                button(PromptAction::StopRecording, strings::PROMPT_END_STOP),
            ],
            countdown: Some(Countdown {
                total: countdown,
                on_expire: PromptAction::StopRecording,
                visible: true,
            }),
            ttl: countdown,
        }
    }
}

/// Keys the prompt understands once it has keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Space,
    Escape,
    Tab,
    ShiftTab,
}

/// Global shortcuts that work without moving focus into the prompt, so
/// dictating in another app is never interrupted. Registered only while a
/// prompt is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    /// Ctrl+Alt+Enter: the first (primary) button.
    Primary,
    /// Ctrl+Alt+Esc: close without an answer.
    Dismiss,
    /// Ctrl+Alt+P: move keyboard focus into the prompt (the user asked, so
    /// this is not focus stealing); Tab, Enter and Esc then work.
    FocusWindow,
}

impl Shortcut {
    pub const ALL: [Shortcut; 3] = [Shortcut::Primary, Shortcut::Dismiss, Shortcut::FocusWindow];

    /// Virtual-key code (Win32 `VK_*`) of the key, with Ctrl+Alt held.
    pub fn virtual_key(self) -> u32 {
        match self {
            Shortcut::Primary => 0x0D,     // VK_RETURN
            Shortcut::Dismiss => 0x1B,     // VK_ESCAPE
            Shortcut::FocusWindow => 0x50, // 'P'
        }
    }
}

/// Where a prompt's countdown stands.
#[derive(Debug)]
pub struct PromptModel {
    request: PromptRequest,
    focus: usize,
    shown_at: Instant,
    /// Hover or keyboard focus pauses the countdown.
    paused_since: Option<Instant>,
    paused_total: Duration,
    hovered: bool,
    keyboard_focus: bool,
    done: bool,
}

impl PromptModel {
    pub fn new(request: PromptRequest, now: Instant) -> Self {
        Self {
            request,
            focus: 0,
            shown_at: now,
            paused_since: None,
            paused_total: Duration::ZERO,
            hovered: false,
            keyboard_focus: false,
            done: false,
        }
    }

    pub fn request(&self) -> &PromptRequest {
        &self.request
    }

    /// Index of the button that has focus (starts on the primary one).
    pub fn focused(&self) -> usize {
        self.focus
    }

    pub fn is_paused(&self) -> bool {
        self.paused_since.is_some()
    }

    fn update_pause(&mut self, now: Instant) {
        let wants_pause = self.hovered || self.keyboard_focus;
        match (wants_pause, self.paused_since) {
            (true, None) => self.paused_since = Some(now),
            (false, Some(since)) => {
                self.paused_total += now.saturating_duration_since(since);
                self.paused_since = None;
            }
            _ => {}
        }
    }

    /// The pointer entered or left the prompt. Hover pauses the countdown.
    pub fn set_hover(&mut self, hovered: bool, now: Instant) {
        self.hovered = hovered;
        self.update_pause(now);
    }

    /// The prompt got or lost keyboard focus. Keyboard focus also pauses
    /// the countdown: someone who is reading it should not be timed out.
    pub fn set_keyboard_focus(&mut self, focused: bool, now: Instant) {
        self.keyboard_focus = focused;
        self.update_pause(now);
    }

    /// Time left on the countdown, or `None` without one.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        let countdown = self.request.countdown.as_ref()?;
        let upto = self.paused_since.unwrap_or(now);
        let elapsed = upto
            .saturating_duration_since(self.shown_at)
            .saturating_sub(self.paused_total);
        Some(countdown.total.saturating_sub(elapsed))
    }

    /// Text for a visible countdown ("Optagelsen stopper om 45 s, ..."),
    /// rounded up to whole seconds. The body otherwise.
    pub fn body_text(&self, now: Instant) -> Option<String> {
        match (&self.request.countdown, self.remaining(now)) {
            (Some(c), Some(left)) if c.visible => {
                let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
                Some(strings::prompt_end_body(&format_da::duration(seconds)))
            }
            _ => self.request.body.clone(),
        }
    }

    /// Advance the clock. Returns the outcome when the countdown ran out.
    pub fn tick(&mut self, now: Instant) -> Option<PromptOutcome> {
        if self.done {
            return None;
        }
        let countdown = self.request.countdown.as_ref()?;
        if self.remaining(now)? == Duration::ZERO {
            let action = countdown.on_expire;
            return Some(self.finish(PromptOutcome::Expired(action)));
        }
        None
    }

    /// A button was clicked (or activated by its own accelerator).
    pub fn click(&mut self, index: usize) -> Option<PromptOutcome> {
        let action = self.request.buttons.get(index)?.action;
        Some(self.finish(PromptOutcome::Chosen(action)))
    }

    /// A key while the prompt has keyboard focus.
    pub fn key(&mut self, key: Key) -> Option<PromptOutcome> {
        let count = self.request.buttons.len();
        match key {
            Key::Enter | Key::Space => self.click(self.focus),
            Key::Escape => Some(self.finish(PromptOutcome::Dismissed)),
            Key::Tab if count > 0 => {
                self.focus = (self.focus + 1) % count;
                None
            }
            Key::ShiftTab if count > 0 => {
                self.focus = (self.focus + count - 1) % count;
                None
            }
            _ => None,
        }
    }

    /// A global shortcut. `FocusWindow` answers nothing; the window moves
    /// focus itself.
    pub fn shortcut(&mut self, shortcut: Shortcut) -> Option<PromptOutcome> {
        match shortcut {
            Shortcut::Primary => self.click(0),
            Shortcut::Dismiss => Some(self.finish(PromptOutcome::Dismissed)),
            Shortcut::FocusWindow => None,
        }
    }

    fn finish(&mut self, outcome: PromptOutcome) -> PromptOutcome {
        self.done = true;
        outcome
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// The name a screen reader announces for the window.
    pub fn window_name(&self) -> String {
        format!("{}: {}", strings::PROMPT_WINDOW_NAME, self.request.title)
    }

    /// The name for button `index` (its label; the primary one says so in
    /// the hint, not in the name, so the name stays what is drawn).
    pub fn button_name(&self, index: usize) -> Option<&str> {
        self.request.buttons.get(index).map(|b| b.label.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(request: PromptRequest) -> (PromptModel, Instant) {
        let t0 = Instant::now();
        (PromptModel::new(request, t0), t0)
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn the_three_use_cases_have_danish_buttons() {
        let q = PromptRequest::meeting_question(1);
        let labels: Vec<_> = q.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(q.title, "Er du i et møde?");
        assert_eq!(labels, ["Optag", "Ignorér"]);

        let r = PromptRequest::meeting_reminder(2, 5);
        assert_eq!(r.title, "Mødet starter om 5 min");
        assert_eq!(r.ttl, secs(300));

        let e = PromptRequest::meeting_end(3, secs(60));
        assert_eq!(e.title, "Er mødet slut?");
        assert_eq!(e.buttons[1].action, PromptAction::StopRecording);
    }

    #[test]
    fn enter_answers_with_the_primary_button() {
        let (mut m, _) = model(PromptRequest::meeting_question(1));
        assert_eq!(m.focused(), 0);
        assert_eq!(
            m.key(Key::Enter),
            Some(PromptOutcome::Chosen(PromptAction::Record))
        );
        assert!(m.is_done());
    }

    #[test]
    fn tab_moves_focus_round_and_space_answers_the_focused_button() {
        let (mut m, _) = model(PromptRequest::meeting_question(1));
        assert_eq!(m.key(Key::Tab), None);
        assert_eq!(m.focused(), 1);
        assert_eq!(m.key(Key::Tab), None);
        assert_eq!(m.focused(), 0, "wraps");
        assert_eq!(m.key(Key::ShiftTab), None);
        assert_eq!(m.focused(), 1, "wraps backwards");
        assert_eq!(
            m.key(Key::Space),
            Some(PromptOutcome::Chosen(PromptAction::Ignore))
        );
    }

    #[test]
    fn escape_dismisses_without_an_answer() {
        let (mut m, _) = model(PromptRequest::meeting_end(1, secs(60)));
        assert_eq!(m.key(Key::Escape), Some(PromptOutcome::Dismissed));
    }

    #[test]
    fn every_button_is_reachable_without_a_pointer() {
        // Via focus: Tab to each button and press Enter.
        for request in [
            PromptRequest::meeting_question(1),
            PromptRequest::meeting_reminder(1, 5),
            PromptRequest::meeting_end(1, secs(60)),
        ] {
            let actions: Vec<_> = request.buttons.iter().map(|b| b.action).collect();
            for (i, want) in actions.iter().enumerate() {
                let (mut m, _) = model(request.clone());
                for _ in 0..i {
                    m.key(Key::Tab);
                }
                assert_eq!(m.key(Key::Enter), Some(PromptOutcome::Chosen(*want)));
            }
        }
    }

    #[test]
    fn global_shortcuts_work_without_focus() {
        let (mut m, _) = model(PromptRequest::meeting_reminder(1, 5));
        assert_eq!(m.shortcut(Shortcut::FocusWindow), None);
        assert!(!m.is_done());
        assert_eq!(
            m.shortcut(Shortcut::Primary),
            Some(PromptOutcome::Chosen(PromptAction::Record))
        );
        let (mut m, _) = model(PromptRequest::meeting_reminder(1, 5));
        assert_eq!(
            m.shortcut(Shortcut::Dismiss),
            Some(PromptOutcome::Dismissed)
        );
    }

    #[test]
    fn click_out_of_range_does_nothing() {
        let (mut m, _) = model(PromptRequest::meeting_question(1));
        assert_eq!(m.click(5), None);
        assert!(!m.is_done());
    }

    #[test]
    fn countdown_expires_with_its_action() {
        let (mut m, t0) = model(PromptRequest::meeting_end(1, secs(30)));
        assert_eq!(m.remaining(t0), Some(secs(30)));
        assert_eq!(m.tick(t0 + secs(29)), None);
        assert_eq!(
            m.tick(t0 + secs(30)),
            Some(PromptOutcome::Expired(PromptAction::StopRecording))
        );
        assert_eq!(m.tick(t0 + secs(31)), None, "only once");
    }

    #[test]
    fn hover_pauses_the_countdown_and_leaving_resumes_it() {
        let (mut m, t0) = model(PromptRequest::meeting_end(1, secs(30)));
        m.set_hover(true, t0 + secs(10));
        assert!(m.is_paused());
        assert_eq!(m.remaining(t0 + secs(100)), Some(secs(20)));
        assert_eq!(m.tick(t0 + secs(100)), None);
        m.set_hover(false, t0 + secs(100));
        assert_eq!(m.remaining(t0 + secs(110)), Some(secs(10)));
        assert!(m.tick(t0 + secs(120)).is_some());
    }

    #[test]
    fn keyboard_focus_pauses_too_and_both_must_end_to_resume() {
        let (mut m, t0) = model(PromptRequest::meeting_end(1, secs(30)));
        m.set_hover(true, t0 + secs(5));
        m.set_keyboard_focus(true, t0 + secs(6));
        m.set_hover(false, t0 + secs(7));
        assert!(m.is_paused(), "still focused");
        m.set_keyboard_focus(false, t0 + secs(50));
        assert_eq!(m.remaining(t0 + secs(50)), Some(secs(25)));
    }

    #[test]
    fn visible_countdown_text_rounds_up() {
        let (m, t0) = model(PromptRequest::meeting_end(1, secs(30)));
        assert_eq!(
            m.body_text(t0).unwrap(),
            "Optagelsen stopper om 30 s, hvis du ikke svarer."
        );
        assert_eq!(
            m.body_text(t0 + Duration::from_millis(500)).unwrap(),
            "Optagelsen stopper om 30 s, hvis du ikke svarer."
        );
        assert_eq!(
            m.body_text(t0 + Duration::from_millis(29_100)).unwrap(),
            "Optagelsen stopper om 1 s, hvis du ikke svarer."
        );
    }

    #[test]
    fn an_invisible_countdown_leaves_the_body_alone() {
        let (m, t0) = model(PromptRequest::meeting_question(1));
        assert_eq!(
            m.body_text(t0).as_deref(),
            Some("Tingskriver kan optage mødet og lave referat bagefter.")
        );
    }

    #[test]
    fn screen_reader_names() {
        let (m, _) = model(PromptRequest::meeting_question(1));
        assert_eq!(m.window_name(), "Tingskriver spørger: Er du i et møde?");
        assert_eq!(m.button_name(0), Some("Optag"));
        assert_eq!(m.button_name(1), Some("Ignorér"));
        assert_eq!(m.button_name(2), None);
    }

    #[test]
    fn shortcut_keys_are_distinct() {
        let mut keys: Vec<_> = Shortcut::ALL.iter().map(|s| s.virtual_key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Shortcut::ALL.len());
    }
}
