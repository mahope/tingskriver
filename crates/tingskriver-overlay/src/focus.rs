//! Whether a prompt may appear now, decided from Windows' user notification
//! state (`QUERY_USER_NOTIFICATION_STATE`, what `SHQueryUserNotificationState`
//! returns). Pure: the Windows layer only supplies the state, so every case
//! is tested on any OS.
//!
//! What the state covers, and what it does not:
//!
//! - Focus Assist ("Fokusassistent", priority only or alarms only) reports
//!   `QUNS_QUIET_TIME`. Automatic rules (duplicating the display, a game,
//!   a full-screen app) report as quiet time or one of the states below.
//! - A full-screen app that is not Direct3D is `QUNS_BUSY`; a full-screen
//!   Direct3D app (most games) is `QUNS_RUNNING_D3D_FULL_SCREEN`; PowerPoint's
//!   slide show and Windows' presentation settings are `QUNS_PRESENTATION_MODE`.
//! - **Screen sharing is not reported** by this API. Sharing a window in a
//!   meeting app does not make a prompt wait. Sharing a whole screen with a
//!   presentation running in full screen does, because the presentation is.
//!   The manual test plan in `docs/promptvindue.md` covers this.
//!
//! A blocked prompt is deferred and asked again until it is out of date,
//! then dropped. Only quiet time is marked in the pill, so the user can see
//! that something was held back on purpose.

use std::time::Duration;

/// `QUERY_USER_NOTIFICATION_STATE`, by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationState {
    /// 1: screen saver, locked session or fast user switching.
    NotPresent,
    /// 2: a full-screen (non-Direct3D) app.
    Busy,
    /// 3: a full-screen Direct3D app.
    RunningD3dFullScreen,
    /// 4: presentation settings or a slide show.
    PresentationMode,
    /// 5: normal, notifications are welcome.
    AcceptsNotifications,
    /// 6: Focus Assist is on.
    QuietTime,
    /// 7: a Windows Store app is full screen; it accepts notifications.
    App,
    /// A value this code does not know.
    Unknown(i32),
}

impl NotificationState {
    pub fn from_raw(raw: i32) -> Self {
        match raw {
            1 => Self::NotPresent,
            2 => Self::Busy,
            3 => Self::RunningD3dFullScreen,
            4 => Self::PresentationMode,
            5 => Self::AcceptsNotifications,
            6 => Self::QuietTime,
            7 => Self::App,
            other => Self::Unknown(other),
        }
    }

    /// True when the user has asked not to be disturbed or cannot be.
    pub fn blocks_prompts(self) -> bool {
        !matches!(
            self,
            Self::AcceptsNotifications | Self::App | Self::Unknown(_)
        )
    }
}

/// How long a deferred prompt waits before the state is checked again.
pub const RETRY_EVERY: Duration = Duration::from_secs(5);

/// What to do with a prompt that wants to appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Show it now.
    Show,
    /// Not now; ask again after `retry_in`. `mark_pill` when the pill should
    /// show that a prompt is held back (quiet time).
    Defer { retry_in: Duration, mark_pill: bool },
    /// Out of date while it waited; never show it. `mark_pill` as above.
    Drop { mark_pill: bool },
}

/// Decide for a prompt that has waited `waited` of its `ttl` (the time after
/// which the question is moot, for example the meeting start). A prompt
/// that is out of date is dropped whatever the state is.
///
/// `state` is `None` when it could not be read. That fails open: a broken
/// query must not swallow every prompt.
pub fn decide(state: Option<NotificationState>, waited: Duration, ttl: Duration) -> Decision {
    let blocked = state.filter(|s| s.blocks_prompts());
    let mark_pill = blocked == Some(NotificationState::QuietTime);
    // A question that is moot is never shown, blocked or not (it may also
    // have waited behind another prompt).
    if waited >= ttl {
        return Decision::Drop { mark_pill };
    }
    if blocked.is_none() {
        return Decision::Show;
    }
    Decision::Defer {
        retry_in: RETRY_EVERY.min(ttl - waited),
        mark_pill,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use NotificationState::*;

    const TTL: Duration = Duration::from_secs(60);
    const ZERO: Duration = Duration::ZERO;

    #[test]
    fn raw_values_match_the_windows_enum() {
        let names = [
            (1, NotPresent),
            (2, Busy),
            (3, RunningD3dFullScreen),
            (4, PresentationMode),
            (5, AcceptsNotifications),
            (6, QuietTime),
            (7, App),
        ];
        for (raw, state) in names {
            assert_eq!(NotificationState::from_raw(raw), state);
        }
        assert_eq!(NotificationState::from_raw(0), Unknown(0));
        assert_eq!(NotificationState::from_raw(99), Unknown(99));
    }

    #[test]
    fn normal_state_shows() {
        for state in [AcceptsNotifications, App] {
            assert_eq!(decide(Some(state), ZERO, TTL), Decision::Show);
        }
    }

    #[test]
    fn quiet_time_defers_and_marks_the_pill() {
        assert_eq!(
            decide(Some(QuietTime), ZERO, TTL),
            Decision::Defer {
                retry_in: RETRY_EVERY,
                mark_pill: true
            }
        );
    }

    #[test]
    fn other_blocking_states_defer_without_a_mark() {
        for state in [NotPresent, Busy, RunningD3dFullScreen, PresentationMode] {
            assert_eq!(
                decide(Some(state), ZERO, TTL),
                Decision::Defer {
                    retry_in: RETRY_EVERY,
                    mark_pill: false
                },
                "{state:?}"
            );
        }
    }

    #[test]
    fn a_deferred_prompt_is_dropped_when_it_is_out_of_date() {
        assert_eq!(
            decide(Some(QuietTime), TTL, TTL),
            Decision::Drop { mark_pill: true }
        );
        assert_eq!(
            decide(Some(Busy), TTL + Duration::from_secs(1), TTL),
            Decision::Drop { mark_pill: false }
        );
    }

    #[test]
    fn the_last_retry_does_not_overshoot_the_ttl() {
        assert_eq!(
            decide(Some(QuietTime), Duration::from_secs(58), TTL),
            Decision::Defer {
                retry_in: Duration::from_secs(2),
                mark_pill: true
            }
        );
    }

    #[test]
    fn prompts_show_once_the_block_ends_even_after_waiting() {
        assert_eq!(
            decide(Some(AcceptsNotifications), Duration::from_secs(30), TTL),
            Decision::Show
        );
    }

    #[test]
    fn an_unreadable_or_unknown_state_fails_open() {
        assert_eq!(decide(None, ZERO, TTL), Decision::Show);
        assert_eq!(decide(Some(Unknown(42)), ZERO, TTL), Decision::Show);
    }

    #[test]
    fn a_moot_prompt_is_dropped_even_when_nothing_blocks() {
        assert_eq!(
            decide(Some(AcceptsNotifications), TTL, TTL),
            Decision::Drop { mark_pill: false }
        );
        assert_eq!(decide(None, TTL, TTL), Decision::Drop { mark_pill: false });
    }

    #[test]
    fn a_zero_ttl_while_blocked_is_dropped_at_once() {
        assert_eq!(
            decide(Some(QuietTime), ZERO, ZERO),
            Decision::Drop { mark_pill: true }
        );
    }
}
