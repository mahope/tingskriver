//! Decide which playback device the system-audio loopback should follow.
//!
//! Windows' default output is not always where a meeting app plays: Teams or
//! Zoom can be set to a headset while the default stays on the speakers. A
//! loopback on the default device is then silent and the remote voices are
//! lost. The platform layer polls which render devices have an active audio
//! session from another process (see `windows_render_sessions`) and feeds the
//! result to [`RenderSelector`], which answers with the device to follow.
//!
//! Plain `std` only, so it is unit tested without audio hardware.
//!
//! Rules:
//! - The default device wins ties: if it is playing, nothing changes.
//! - Only when the default is idle and another device is playing does the
//!   loopback move, and only after [`SWITCH_CONFIRMATIONS`] polls in a row
//!   that agree.
//! - Moving back to the default needs the same confirmations, so a short
//!   system sound on the default does not make the loopback flap.
//! - A poll where nothing plays anywhere says nothing: the choice is kept
//!   (a pause in the call must not move the loopback).

use std::time::Duration;

/// How often the playback sessions are polled.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Consecutive polls that must agree before the loopback moves.
pub const SWITCH_CONFIRMATIONS: u32 = 2;
/// Longest stretch of silence allowed when the loopback moves to another
/// device. The new loopback is opened before the old one is stopped, so the
/// real gap is normally zero; this is the bound the follower holds itself to.
pub const MAX_SWITCH_GAP: Duration = Duration::from_secs(1);

/// What one poll saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderObservation {
    /// Current Windows default output device.
    pub default: Option<String>,
    /// Render devices with an active session from a process other than ours.
    pub active: Vec<String>,
}

/// Where a single poll says the loopback should be.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    /// Nothing plays anywhere: no information.
    NoSignal,
    /// The default device is playing, or nothing but the default is.
    Default,
    /// The default is idle and this other device is playing.
    Other(String),
}

fn judge(obs: &RenderObservation, current: Option<&str>) -> Verdict {
    if obs.active.is_empty() {
        return Verdict::NoSignal;
    }
    if let Some(default) = obs.default.as_deref() {
        if obs.active.iter().any(|d| d == default) {
            return Verdict::Default;
        }
    }
    // Several other devices play: stay where we are if that is one of them,
    // else the first by name so the pick does not depend on enumeration order.
    if let Some(current) = current {
        if obs.active.iter().any(|d| d == current) {
            return Verdict::Other(current.to_string());
        }
    }
    let mut others: Vec<&String> = obs.active.iter().collect();
    others.sort();
    Verdict::Other(others[0].clone())
}

/// A pending change and how many polls in a row have asked for it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    /// `None`: back to the default.
    target: Option<String>,
    seen: u32,
}

/// Confirms playback-device changes over several polls.
#[derive(Debug, Clone, Default)]
pub struct RenderSelector {
    /// Device the loopback follows instead of the default (`None`: default).
    current: Option<String>,
    pending: Option<Pending>,
}

impl RenderSelector {
    pub fn new() -> Self {
        Self::default()
    }

    /// The device followed instead of the default, if any.
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Feed one poll. Returns the device to follow instead of the default
    /// (`None`: follow the default).
    pub fn observe(&mut self, obs: &RenderObservation) -> Option<&str> {
        let wanted: Option<String> = match judge(obs, self.current.as_deref()) {
            Verdict::NoSignal => {
                self.pending = None;
                return self.current.as_deref();
            }
            Verdict::Default => None,
            Verdict::Other(device) => Some(device),
        };
        if wanted == self.current {
            self.pending = None;
            return self.current.as_deref();
        }
        let seen = match &self.pending {
            Some(p) if p.target == wanted => p.seen + 1,
            _ => 1,
        };
        if seen >= SWITCH_CONFIRMATIONS {
            self.current = wanted;
            self.pending = None;
        } else {
            self.pending = Some(Pending {
                target: wanted,
                seen,
            });
        }
        self.current.as_deref()
    }

    /// Forget the choice (recording restarted, or the followed device is
    /// gone).
    pub fn reset(&mut self) {
        self.current = None;
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEAKERS: &str = "Speakers (Realtek(R) Audio)";
    const HEADSET: &str = "Headset Earphone (Jabra Evolve2 65)";
    const MONITOR: &str = "DELL U2720Q (NVIDIA High Definition Audio)";

    fn obs(default: &str, active: &[&str]) -> RenderObservation {
        RenderObservation {
            default: Some(default.to_string()),
            active: active.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn default_idle_other_active_switches_after_two_polls() {
        let mut s = RenderSelector::new();
        assert_eq!(
            s.observe(&obs(SPEAKERS, &[HEADSET])),
            None,
            "one poll is not enough"
        );
        assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET])), Some(HEADSET));
    }

    #[test]
    fn default_active_never_switches() {
        let mut s = RenderSelector::new();
        for _ in 0..5 {
            assert_eq!(s.observe(&obs(SPEAKERS, &[SPEAKERS])), None);
        }
    }

    #[test]
    fn tie_goes_to_the_default() {
        let mut s = RenderSelector::new();
        for _ in 0..5 {
            assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET, SPEAKERS])), None);
        }
    }

    #[test]
    fn tie_moves_back_to_default_after_two_polls() {
        let mut s = RenderSelector::new();
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET])), Some(HEADSET));
        // Both play: the default wins, but only once confirmed.
        assert_eq!(
            s.observe(&obs(SPEAKERS, &[HEADSET, SPEAKERS])),
            Some(HEADSET)
        );
        assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET, SPEAKERS])), None);
    }

    #[test]
    fn a_single_blip_does_not_switch() {
        let mut s = RenderSelector::new();
        assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET])), None);
        // The next poll disagrees: the count starts over.
        assert_eq!(s.observe(&obs(SPEAKERS, &[SPEAKERS])), None);
        assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET])), None);
        assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET])), Some(HEADSET));
    }

    #[test]
    fn a_short_system_sound_on_the_default_does_not_flap() {
        let mut s = RenderSelector::new();
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        assert_eq!(s.current(), Some(HEADSET));
        // One poll where only the default plays (a notification sound).
        assert_eq!(s.observe(&obs(SPEAKERS, &[SPEAKERS])), Some(HEADSET));
        assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET])), Some(HEADSET));
    }

    #[test]
    fn silence_everywhere_keeps_the_choice() {
        let mut s = RenderSelector::new();
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        for _ in 0..5 {
            assert_eq!(s.observe(&obs(SPEAKERS, &[])), Some(HEADSET));
        }
    }

    #[test]
    fn silence_everywhere_is_not_a_confirmation() {
        let mut s = RenderSelector::new();
        assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET])), None);
        assert_eq!(s.observe(&obs(SPEAKERS, &[])), None);
        assert_eq!(
            s.observe(&obs(SPEAKERS, &[HEADSET])),
            None,
            "count restarted"
        );
    }

    #[test]
    fn default_that_is_not_listed_still_follows_active_device() {
        let mut s = RenderSelector::new();
        let o = RenderObservation {
            default: None,
            active: vec![HEADSET.to_string()],
        };
        s.observe(&o);
        assert_eq!(s.observe(&o), Some(HEADSET));
    }

    #[test]
    fn several_other_devices_pick_is_stable() {
        let mut s = RenderSelector::new();
        // Enumeration order flips between polls; the pick must not.
        s.observe(&obs(SPEAKERS, &[MONITOR, HEADSET]));
        assert_eq!(
            s.observe(&obs(SPEAKERS, &[HEADSET, MONITOR])),
            Some(MONITOR)
        );
    }

    #[test]
    fn stays_on_current_device_while_it_still_plays_with_another() {
        let mut s = RenderSelector::new();
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        for _ in 0..4 {
            assert_eq!(
                s.observe(&obs(SPEAKERS, &[MONITOR, HEADSET])),
                Some(HEADSET)
            );
        }
    }

    #[test]
    fn moves_between_two_non_default_devices_after_two_polls() {
        let mut s = RenderSelector::new();
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        assert_eq!(s.observe(&obs(SPEAKERS, &[MONITOR])), Some(HEADSET));
        assert_eq!(s.observe(&obs(SPEAKERS, &[MONITOR])), Some(MONITOR));
    }

    #[test]
    fn reset_forgets_everything() {
        let mut s = RenderSelector::new();
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        s.observe(&obs(SPEAKERS, &[HEADSET]));
        s.reset();
        assert_eq!(s.current(), None);
        assert_eq!(s.observe(&obs(SPEAKERS, &[HEADSET])), None);
    }

    #[test]
    fn silence_bound_is_one_second() {
        assert_eq!(MAX_SWITCH_GAP, Duration::from_secs(1));
        assert_eq!(SWITCH_CONFIRMATIONS, 2);
        assert_eq!(POLL_INTERVAL, Duration::from_secs(2));
    }
}
