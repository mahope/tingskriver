//! Platform-independent decision logic for following the OS default audio
//! devices during a recording (used by the Windows hot-swap follower).
//!
//! Everything here is plain `std` so it can be unit tested without audio
//! hardware: a [`DeviceSource`] reports which devices exist and which ones are
//! the defaults, and [`DeviceFollower`] decides which stream must be reopened
//! on which device. The platform layer performs the actual stream swap and
//! reports the outcome back via [`DeviceFollower::swap_succeeded`] /
//! [`DeviceFollower::swap_failed`].

use std::fmt;
use std::time::Duration;

/// The two capture streams of a recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamKind {
    /// Capture endpoint (the user's microphone).
    Microphone,
    /// Render endpoint captured via loopback (what the PC plays).
    System,
}

impl fmt::Display for StreamKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamKind::Microphone => write!(f, "microphone"),
            StreamKind::System => write!(f, "system audio"),
        }
    }
}

/// What the OS currently reports for one stream kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceSnapshot {
    /// Name of the current default device, if any.
    pub default_name: Option<String>,
    /// Names of all devices that are currently present/active.
    pub present: Vec<String>,
}

impl DeviceSnapshot {
    fn is_present(&self, name: &str) -> bool {
        self.present.iter().any(|n| n == name)
    }
}

/// Source of device information. Implemented with WASAPI on Windows and by a
/// fake in the tests.
pub trait DeviceSource {
    fn snapshot(&self, kind: StreamKind) -> Result<DeviceSnapshot, String>;
}

/// Why a stream is being moved to another device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchReason {
    /// The default device changed while we were following the default.
    DefaultChanged,
    /// The device we were recording from is gone.
    ActiveDisappeared,
    /// The device the user explicitly picked is back.
    PreferredReturned,
    /// The stream is not running (it failed to open earlier or a swap failed).
    StreamMissing,
    /// The device is still listed (and may still be the default) but delivers
    /// nothing but silence, e.g. a USB dongle whose headset is switched off.
    EndpointSilent,
}

impl fmt::Display for SwitchReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            SwitchReason::DefaultChanged => "default device changed",
            SwitchReason::ActiveDisappeared => "active device disappeared",
            SwitchReason::PreferredReturned => "selected device returned",
            SwitchReason::StreamMissing => "stream not running",
            SwitchReason::EndpointSilent => "device delivers only silence",
        };
        f.write_str(text)
    }
}

/// A stream that must be (re)opened on another device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchAction {
    pub kind: StreamKind,
    /// Device the stream currently uses (`None` if no stream is running).
    pub from: Option<String>,
    pub to: String,
    pub reason: SwitchReason,
}

/// Result of evaluating the device state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Evaluation {
    /// Streams to reopen now.
    pub switches: Vec<SwitchAction>,
    /// The active device is gone but the OS has not picked a new default yet
    /// (or there is no device at all). The stream should be torn down and the
    /// caller should evaluate again shortly.
    pub lost: Vec<(StreamKind, Option<String>)>,
    /// True if the caller should schedule another evaluation (something is
    /// still unresolved: lost streams or failed swaps).
    pub retry: bool,
}

/// Tracks one stream (microphone or system audio).
#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamTracker {
    kind: StreamKind,
    /// `None`: follow the OS default. `Some(name)`: the user explicitly picked
    /// this device; use it whenever it is present, else fall back to default.
    preferred: Option<String>,
    /// Device the running stream is bound to (`None`: no stream running).
    active: Option<String>,
    /// Consecutive failed swap attempts (reset on success).
    failures: u32,
    /// A device that is listed but known to deliver only silence; skipped as
    /// a target while another device is available.
    dead: Option<String>,
}

impl StreamTracker {
    fn is_dead(&self, name: &str) -> bool {
        self.dead.as_deref() == Some(name)
    }

    fn target(&self, snap: &DeviceSnapshot) -> Option<String> {
        if let Some(pref) = &self.preferred {
            if snap.is_present(pref) && !self.is_dead(pref) {
                return Some(pref.clone());
            }
        }
        match &snap.default_name {
            Some(default) if !self.is_dead(default) => Some(default.clone()),
            // The default is dead: use any other device that is there. If
            // none is, stay on the default rather than on nothing.
            default => snap
                .present
                .iter()
                .find(|n| !self.is_dead(n))
                .cloned()
                .or_else(|| default.clone()),
        }
    }

    fn evaluate(&self, snap: &DeviceSnapshot, eval: &mut Evaluation) {
        let target = self.target(snap);
        let active_present = self
            .active
            .as_deref()
            .map(|a| snap.is_present(a))
            .unwrap_or(false);

        match (&self.active, target) {
            // Running on the right device: nothing to do.
            (Some(active), Some(target)) if *active == target && active_present => {}
            // Windows can still report the vanished device as default for a
            // moment after removal. Tear the dead stream down and look again.
            (Some(active), Some(target)) if *active == target => {
                eval.lost.push((self.kind, Some(active.clone())));
                eval.retry = true;
            }
            (Some(active), Some(target)) => {
                let reason = if !active_present {
                    SwitchReason::ActiveDisappeared
                } else if self.is_dead(active) {
                    SwitchReason::EndpointSilent
                } else if self.preferred.as_deref() == Some(target.as_str()) {
                    SwitchReason::PreferredReturned
                } else {
                    SwitchReason::DefaultChanged
                };
                eval.switches.push(SwitchAction {
                    kind: self.kind,
                    from: Some(active.clone()),
                    to: target,
                    reason,
                });
            }
            (None, Some(target)) => {
                eval.switches.push(SwitchAction {
                    kind: self.kind,
                    from: None,
                    to: target,
                    reason: SwitchReason::StreamMissing,
                });
            }
            // No device to switch to. Keep a live stream; drop a dead one.
            (Some(active), None) => {
                if !active_present {
                    eval.lost.push((self.kind, Some(active.clone())));
                    eval.retry = true;
                }
            }
            (None, None) => {
                eval.retry = true;
            }
        }
    }
}

/// Decides which streams must move when devices or defaults change.
#[derive(Debug, Clone)]
pub struct DeviceFollower {
    trackers: Vec<StreamTracker>,
}

impl DeviceFollower {
    /// `mic_preferred` / `system_preferred`: `None` to follow the OS default,
    /// `Some(name)` for a device the user explicitly selected.
    /// `mic_active` / `system_active`: devices the streams were opened on, or
    /// `None` if that stream is not running. A kind is only tracked if it is
    /// part of the recording (`track_*`).
    pub fn new(
        track_mic: bool,
        mic_preferred: Option<String>,
        mic_active: Option<String>,
        track_system: bool,
        system_preferred: Option<String>,
        system_active: Option<String>,
    ) -> Self {
        let mut trackers = Vec::new();
        if track_mic {
            trackers.push(StreamTracker {
                kind: StreamKind::Microphone,
                preferred: mic_preferred,
                active: mic_active,
                failures: 0,
                dead: None,
            });
        }
        if track_system {
            trackers.push(StreamTracker {
                kind: StreamKind::System,
                preferred: system_preferred,
                active: system_active,
                failures: 0,
                dead: None,
            });
        }
        Self { trackers }
    }

    /// Kinds this follower manages.
    pub fn kinds(&self) -> Vec<StreamKind> {
        self.trackers.iter().map(|t| t.kind).collect()
    }

    /// Device the stream of `kind` is bound to, if a stream is running.
    pub fn active(&self, kind: StreamKind) -> Option<&str> {
        self.tracker(kind).and_then(|t| t.active.as_deref())
    }

    /// Evaluate the current device state. Snapshot errors for a kind are
    /// treated as "try again later".
    pub fn evaluate(&self, source: &dyn DeviceSource) -> Evaluation {
        let mut eval = Evaluation::default();
        for tracker in &self.trackers {
            match source.snapshot(tracker.kind) {
                Ok(snap) => tracker.evaluate(&snap, &mut eval),
                Err(_) => eval.retry = true,
            }
        }
        eval
    }

    /// The stream of `kind` was torn down (device lost, no replacement yet).
    pub fn stream_lost(&mut self, kind: StreamKind) {
        if let Some(t) = self.tracker_mut(kind) {
            t.active = None;
        }
    }

    /// The device of `kind` is listed but delivers only silence: stop
    /// targeting it until [`clear_dead`](Self::clear_dead).
    pub fn mark_dead(&mut self, kind: StreamKind, device: &str) {
        if let Some(t) = self.tracker_mut(kind) {
            t.dead = Some(device.to_string());
        }
    }

    /// Target the device again (Windows reported a change, or the wait ran out).
    pub fn clear_dead(&mut self, kind: StreamKind) {
        if let Some(t) = self.tracker_mut(kind) {
            t.dead = None;
        }
    }

    /// The device of `kind` currently marked as silent, if any.
    pub fn dead(&self, kind: StreamKind) -> Option<&str> {
        self.tracker(kind).and_then(|t| t.dead.as_deref())
    }

    pub fn swap_succeeded(&mut self, kind: StreamKind, device: &str) {
        if let Some(t) = self.tracker_mut(kind) {
            t.active = Some(device.to_string());
            t.failures = 0;
        }
    }

    /// A swap failed. The old stream is gone either way, so the kind is now
    /// without a stream; the next evaluation retries. Returns the delay to
    /// wait before retrying.
    pub fn swap_failed(&mut self, kind: StreamKind) -> Duration {
        match self.tracker_mut(kind) {
            Some(t) => {
                t.active = None;
                t.failures = t.failures.saturating_add(1);
                retry_delay(t.failures)
            }
            None => retry_delay(1),
        }
    }

    /// Consecutive failed swap attempts for `kind` since the last success.
    pub fn failures(&self, kind: StreamKind) -> u32 {
        self.tracker(kind).map_or(0, |t| t.failures)
    }

    fn tracker(&self, kind: StreamKind) -> Option<&StreamTracker> {
        self.trackers.iter().find(|t| t.kind == kind)
    }

    fn tracker_mut(&mut self, kind: StreamKind) -> Option<&mut StreamTracker> {
        self.trackers.iter_mut().find(|t| t.kind == kind)
    }
}

/// Failed attempts in a row after which the user is told the stream could
/// not be recovered (retries continue in the background).
pub const FAILURES_BEFORE_EXHAUSTED: u32 = 3;

/// What to tell the user after a failed swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureNotice {
    /// Nothing new to say; retrying quietly.
    None,
    /// First failure in a row: the switch did not work.
    SwapFailed,
    /// Still failing after `FAILURES_BEFORE_EXHAUSTED` attempts.
    RecoveryExhausted,
}

/// Notify once when switching starts failing and once when it keeps failing,
/// not on every retry.
pub fn failure_notice(failures: u32) -> FailureNotice {
    match failures {
        1 => FailureNotice::SwapFailed,
        n if n == FAILURES_BEFORE_EXHAUSTED => FailureNotice::RecoveryExhausted,
        _ => FailureNotice::None,
    }
}

/// Backoff between failed swap attempts: 0.5 s, 1 s, 2 s, 4 s, then 5 s.
pub fn retry_delay(failures: u32) -> Duration {
    let ms = 500u64.saturating_mul(1u64 << failures.saturating_sub(1).min(4));
    Duration::from_millis(ms.min(5_000))
}

/// Peak amplitude at or below which a buffer counts as silent. A live
/// microphone always has a noise floor above this (one 16-bit step is ~3e-5);
/// a dead endpoint delivers exact zeros.
pub const SILENCE_PEAK: f32 = 1.0e-5;
/// How long a microphone may deliver only silence before its endpoint is
/// considered dead and another device is used.
pub const DEAD_ENDPOINT_AFTER: Duration = Duration::from_secs(1);
/// How long a device stays excluded if Windows reports no change for it.
pub const DEAD_RETRY_AFTER: Duration = Duration::from_secs(120);

/// True if a microphone that has been silent for `silent_for` should be
/// treated as dead.
pub fn endpoint_is_dead(silent_for: Duration) -> bool {
    silent_for >= DEAD_ENDPOINT_AFTER
}

/// Windows-1252 characters for the bytes 0x80..=0x9F (0 = undefined).
const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC,
    0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
];

/// Undo UTF-8 text that was decoded as Windows-1252/Latin-1 and encoded as
/// UTF-8 again ("pÃ¥" -> "på"). Text that is not such a double encoding is
/// returned unchanged.
pub fn repair_double_utf8(text: &str) -> String {
    if text.is_ascii() {
        return text.to_string();
    }
    let mut bytes = Vec::with_capacity(text.len());
    for c in text.chars() {
        let code = c as u32;
        let byte = if code < 0x80 || (0xA0..=0xFF).contains(&code) {
            code as u8
        } else if let Some(i) = CP1252_HIGH.iter().position(|&h| h as u32 == code && h != 0) {
            0x80 + i as u8
        } else if (0x80..=0x9F).contains(&code) {
            code as u8
        } else {
            return text.to_string();
        };
        bytes.push(byte);
    }
    match String::from_utf8(bytes) {
        // Only a real repair if it actually shortened the text.
        Ok(fixed) if fixed.chars().count() < text.chars().count() => fixed,
        _ => text.to_string(),
    }
}

/// Number of silence samples to emit so that a stream that has been missing
/// for `elapsed` is filled up to real time, given `emitted` samples so far.
/// Driven by elapsed wall-clock time, so timer jitter does not accumulate.
pub fn silence_samples_due(elapsed: Duration, emitted: u64, sample_rate: u32) -> u64 {
    let expected = (elapsed.as_secs_f64() * sample_rate as f64) as u64;
    expected.saturating_sub(emitted)
}

/// One line describing a completed or failed switch, for the log.
pub fn describe_switch(action: &SwitchAction, took: Duration, outcome: Result<(), &str>) -> String {
    let from = action.from.as_deref().unwrap_or("<none>");
    match outcome {
        Ok(()) => format!(
            "{} switched '{}' -> '{}' ({}), gap filled with silence, took {} ms",
            action.kind,
            from,
            action.to,
            action.reason,
            took.as_millis()
        ),
        Err(e) => format!(
            "{} switch '{}' -> '{}' ({}) FAILED after {} ms: {}",
            action.kind,
            from,
            action.to,
            action.reason,
            took.as_millis(),
            e
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    const LAPTOP_MIC: &str = "Microphone Array (Realtek(R) Audio)";
    const SPEAKERS: &str = "Speakers (Realtek(R) Audio)";
    const HEADSET_MIC: &str = "Headset Microphone (Jabra Evolve2 65)";
    const HEADSET_OUT: &str = "Headset Earphone (Jabra Evolve2 65)";
    const USB_MIC: &str = "Yeti Stereo Microphone";

    /// Fake device source: a mutable view of what "Windows" reports.
    #[derive(Default)]
    struct FakeDevices {
        state: RefCell<HashMap<StreamKind, DeviceSnapshot>>,
        failing: RefCell<Vec<StreamKind>>,
    }

    impl FakeDevices {
        fn new(mics: &[&str], mic_default: &str, outs: &[&str], out_default: &str) -> Self {
            let fake = FakeDevices::default();
            fake.set(StreamKind::Microphone, mics, Some(mic_default));
            fake.set(StreamKind::System, outs, Some(out_default));
            fake
        }

        fn set(&self, kind: StreamKind, present: &[&str], default: Option<&str>) {
            self.state.borrow_mut().insert(
                kind,
                DeviceSnapshot {
                    default_name: default.map(str::to_string),
                    present: present.iter().map(|s| s.to_string()).collect(),
                },
            );
        }

        fn unplug_headset(&self) {
            self.set(StreamKind::Microphone, &[LAPTOP_MIC], Some(LAPTOP_MIC));
            self.set(StreamKind::System, &[SPEAKERS], Some(SPEAKERS));
        }

        fn plug_headset(&self) {
            self.set(
                StreamKind::Microphone,
                &[LAPTOP_MIC, HEADSET_MIC],
                Some(HEADSET_MIC),
            );
            self.set(
                StreamKind::System,
                &[SPEAKERS, HEADSET_OUT],
                Some(HEADSET_OUT),
            );
        }
    }

    impl DeviceSource for FakeDevices {
        fn snapshot(&self, kind: StreamKind) -> Result<DeviceSnapshot, String> {
            if self.failing.borrow().contains(&kind) {
                return Err("enumeration failed".into());
            }
            Ok(self.state.borrow().get(&kind).cloned().unwrap_or_default())
        }
    }

    /// Applies an evaluation the way the platform layer does, assuming every
    /// swap succeeds. Returns the switches that were performed.
    fn apply(follower: &mut DeviceFollower, eval: &Evaluation) -> Vec<SwitchAction> {
        for (kind, _) in &eval.lost {
            follower.stream_lost(*kind);
        }
        for s in &eval.switches {
            follower.swap_succeeded(s.kind, &s.to);
        }
        eval.switches.clone()
    }

    fn following_headset() -> (FakeDevices, DeviceFollower) {
        let fake = FakeDevices::new(
            &[LAPTOP_MIC, HEADSET_MIC],
            HEADSET_MIC,
            &[SPEAKERS, HEADSET_OUT],
            HEADSET_OUT,
        );
        let follower = DeviceFollower::new(
            true,
            None,
            Some(HEADSET_MIC.into()),
            true,
            None,
            Some(HEADSET_OUT.into()),
        );
        (fake, follower)
    }

    #[test]
    fn steady_state_does_nothing() {
        let (fake, follower) = following_headset();
        assert_eq!(follower.evaluate(&fake), Evaluation::default());
    }

    #[test]
    fn headset_unplugged_moves_both_streams_to_new_defaults() {
        let (fake, mut follower) = following_headset();
        fake.unplug_headset();

        let eval = follower.evaluate(&fake);
        assert!(!eval.retry);
        let switches = apply(&mut follower, &eval);
        assert_eq!(
            switches,
            vec![
                SwitchAction {
                    kind: StreamKind::Microphone,
                    from: Some(HEADSET_MIC.into()),
                    to: LAPTOP_MIC.into(),
                    reason: SwitchReason::ActiveDisappeared,
                },
                SwitchAction {
                    kind: StreamKind::System,
                    from: Some(HEADSET_OUT.into()),
                    to: SPEAKERS.into(),
                    reason: SwitchReason::ActiveDisappeared,
                },
            ]
        );
        assert_eq!(follower.active(StreamKind::Microphone), Some(LAPTOP_MIC));
        assert_eq!(follower.active(StreamKind::System), Some(SPEAKERS));
        // Settled: nothing more to do.
        assert_eq!(follower.evaluate(&fake), Evaluation::default());
    }

    #[test]
    fn headset_back_as_default_switches_back_no_sticky_fallback() {
        let (fake, mut follower) = following_headset();
        fake.unplug_headset();
        let eval = follower.evaluate(&fake);
        apply(&mut follower, &eval);

        fake.plug_headset();
        let eval = follower.evaluate(&fake);
        let switches = apply(&mut follower, &eval);
        assert_eq!(switches.len(), 2);
        assert!(switches
            .iter()
            .all(|s| s.reason == SwitchReason::DefaultChanged));
        assert_eq!(follower.active(StreamKind::Microphone), Some(HEADSET_MIC));
        assert_eq!(follower.active(StreamKind::System), Some(HEADSET_OUT));
    }

    #[test]
    fn two_unplug_replug_cycles() {
        let (fake, mut follower) = following_headset();
        let mut total = 0;
        for _ in 0..2 {
            fake.unplug_headset();
            let eval = follower.evaluate(&fake);
            total += apply(&mut follower, &eval).len();
            fake.plug_headset();
            let eval = follower.evaluate(&fake);
            total += apply(&mut follower, &eval).len();
        }
        assert_eq!(total, 8);
        assert_eq!(follower.evaluate(&fake), Evaluation::default());
    }

    #[test]
    fn headset_present_but_no_longer_default_follows_default() {
        // User switches default output to the speakers in Windows settings
        // while the headset stays connected.
        let (fake, mut follower) = following_headset();
        fake.set(StreamKind::System, &[SPEAKERS, HEADSET_OUT], Some(SPEAKERS));
        let eval = follower.evaluate(&fake);
        let switches = apply(&mut follower, &eval);
        assert_eq!(
            switches,
            vec![SwitchAction {
                kind: StreamKind::System,
                from: Some(HEADSET_OUT.into()),
                to: SPEAKERS.into(),
                reason: SwitchReason::DefaultChanged,
            }]
        );
    }

    #[test]
    fn stale_default_waits_and_tears_down_dead_stream() {
        // Device removed, but Windows still reports it as default.
        let (fake, mut follower) = following_headset();
        fake.set(StreamKind::Microphone, &[LAPTOP_MIC], Some(HEADSET_MIC));
        let eval = follower.evaluate(&fake);
        assert!(eval.switches.is_empty());
        assert_eq!(
            eval.lost,
            vec![(StreamKind::Microphone, Some(HEADSET_MIC.into()))]
        );
        assert!(eval.retry);
        apply(&mut follower, &eval);
        assert_eq!(follower.active(StreamKind::Microphone), None);

        // Windows catches up.
        fake.set(StreamKind::Microphone, &[LAPTOP_MIC], Some(LAPTOP_MIC));
        let eval = follower.evaluate(&fake);
        assert_eq!(
            eval.switches,
            vec![SwitchAction {
                kind: StreamKind::Microphone,
                from: None,
                to: LAPTOP_MIC.into(),
                reason: SwitchReason::StreamMissing,
            }]
        );
    }

    #[test]
    fn no_devices_left_keeps_retrying() {
        let (fake, mut follower) = following_headset();
        fake.set(StreamKind::Microphone, &[], None);
        let eval = follower.evaluate(&fake);
        assert_eq!(
            eval.lost,
            vec![(StreamKind::Microphone, Some(HEADSET_MIC.into()))]
        );
        assert!(eval.retry);
        apply(&mut follower, &eval);
        let eval = follower.evaluate(&fake);
        assert!(eval.switches.is_empty() && eval.lost.is_empty() && eval.retry);
    }

    #[test]
    fn pinned_device_is_kept_when_default_changes() {
        let fake = FakeDevices::new(&[LAPTOP_MIC, USB_MIC], LAPTOP_MIC, &[SPEAKERS], SPEAKERS);
        let mut follower = DeviceFollower::new(
            true,
            Some(USB_MIC.into()),
            Some(USB_MIC.into()),
            true,
            None,
            Some(SPEAKERS.into()),
        );
        fake.set(
            StreamKind::Microphone,
            &[LAPTOP_MIC, USB_MIC, HEADSET_MIC],
            Some(HEADSET_MIC),
        );
        assert_eq!(follower.evaluate(&fake), Evaluation::default());

        // The pinned mic disappears: fall back to the default.
        fake.set(
            StreamKind::Microphone,
            &[LAPTOP_MIC, HEADSET_MIC],
            Some(HEADSET_MIC),
        );
        let eval = follower.evaluate(&fake);
        apply(&mut follower, &eval);
        assert_eq!(follower.active(StreamKind::Microphone), Some(HEADSET_MIC));

        // And it comes back: return to it even though it is not the default.
        fake.set(
            StreamKind::Microphone,
            &[LAPTOP_MIC, USB_MIC, HEADSET_MIC],
            Some(HEADSET_MIC),
        );
        let eval = follower.evaluate(&fake);
        assert_eq!(
            eval.switches,
            vec![SwitchAction {
                kind: StreamKind::Microphone,
                from: Some(HEADSET_MIC.into()),
                to: USB_MIC.into(),
                reason: SwitchReason::PreferredReturned,
            }]
        );
    }

    #[test]
    fn system_stream_that_failed_at_start_is_opened_later() {
        let fake = FakeDevices::new(&[LAPTOP_MIC], LAPTOP_MIC, &[SPEAKERS], SPEAKERS);
        let follower = DeviceFollower::new(true, None, Some(LAPTOP_MIC.into()), true, None, None);
        let eval = follower.evaluate(&fake);
        assert_eq!(
            eval.switches,
            vec![SwitchAction {
                kind: StreamKind::System,
                from: None,
                to: SPEAKERS.into(),
                reason: SwitchReason::StreamMissing,
            }]
        );
    }

    #[test]
    fn untracked_kind_is_ignored() {
        let fake = FakeDevices::new(&[LAPTOP_MIC], LAPTOP_MIC, &[SPEAKERS], SPEAKERS);
        let follower = DeviceFollower::new(true, None, Some(LAPTOP_MIC.into()), false, None, None);
        assert_eq!(follower.kinds(), vec![StreamKind::Microphone]);
        assert_eq!(follower.evaluate(&fake), Evaluation::default());
    }

    #[test]
    fn failed_swap_backs_off_and_retries() {
        let (fake, mut follower) = following_headset();
        fake.unplug_headset();
        let eval = follower.evaluate(&fake);
        let mic = eval.switches[0].clone();

        assert_eq!(follower.swap_failed(mic.kind), Duration::from_millis(500));
        assert_eq!(follower.active(StreamKind::Microphone), None);
        // Next evaluation tries again (stream missing now).
        let eval = follower.evaluate(&fake);
        assert!(eval
            .switches
            .iter()
            .any(|s| s.kind == StreamKind::Microphone && s.reason == SwitchReason::StreamMissing));
        assert_eq!(follower.swap_failed(mic.kind), Duration::from_millis(1000));
        assert_eq!(follower.swap_failed(mic.kind), Duration::from_millis(2000));
        assert_eq!(follower.swap_failed(mic.kind), Duration::from_millis(4000));
        assert_eq!(follower.swap_failed(mic.kind), Duration::from_millis(5000));
        assert_eq!(follower.swap_failed(mic.kind), Duration::from_millis(5000));
        follower.swap_succeeded(mic.kind, &mic.to);
        assert_eq!(follower.swap_failed(mic.kind), Duration::from_millis(500));
    }

    #[test]
    fn enumeration_error_requests_retry_without_switching() {
        let (fake, follower) = following_headset();
        fake.unplug_headset();
        fake.failing.borrow_mut().push(StreamKind::System);
        let eval = follower.evaluate(&fake);
        assert!(eval.retry);
        assert_eq!(eval.switches.len(), 1);
        assert_eq!(eval.switches[0].kind, StreamKind::Microphone);
    }

    #[test]
    fn silence_fill_tracks_wall_clock() {
        let rate = 48_000;
        assert_eq!(silence_samples_due(Duration::from_millis(0), 0, rate), 0);
        assert_eq!(
            silence_samples_due(Duration::from_millis(100), 0, rate),
            4_800
        );
        // Late tick catches up exactly; nothing is double-counted.
        assert_eq!(
            silence_samples_due(Duration::from_millis(350), 4_800, rate),
            12_000
        );
        assert_eq!(
            silence_samples_due(Duration::from_millis(350), 16_800, rate),
            0
        );
        // A 1.5 s gap is filled with 1.5 s of samples in total.
        let mut emitted = 0;
        for ms in (0..=1500).step_by(37) {
            emitted += silence_samples_due(Duration::from_millis(ms), emitted, rate);
        }
        emitted += silence_samples_due(Duration::from_millis(1500), emitted, rate);
        assert_eq!(emitted, 72_000);
    }

    #[test]
    fn switch_description_names_devices_reason_and_duration() {
        let action = SwitchAction {
            kind: StreamKind::System,
            from: Some(HEADSET_OUT.into()),
            to: SPEAKERS.into(),
            reason: SwitchReason::ActiveDisappeared,
        };
        let ok = describe_switch(&action, Duration::from_millis(412), Ok(()));
        assert_eq!(
            ok,
            "system audio switched 'Headset Earphone (Jabra Evolve2 65)' -> 'Speakers (Realtek(R) Audio)' (active device disappeared), gap filled with silence, took 412 ms"
        );
        let err = describe_switch(&action, Duration::from_millis(80), Err("boom"));
        assert!(err.contains("FAILED after 80 ms: boom"));
    }

    #[test]
    fn failed_swaps_notify_once_then_once_more_when_exhausted() {
        let mut follower = DeviceFollower::new(
            true,
            None,
            Some(HEADSET_MIC.into()),
            true,
            None,
            Some(HEADSET_OUT.into()),
        );
        let notices: Vec<FailureNotice> = (0..6)
            .map(|_| {
                follower.swap_failed(StreamKind::Microphone);
                failure_notice(follower.failures(StreamKind::Microphone))
            })
            .collect();
        assert_eq!(
            notices,
            vec![
                FailureNotice::SwapFailed,
                FailureNotice::None,
                FailureNotice::RecoveryExhausted,
                FailureNotice::None,
                FailureNotice::None,
                FailureNotice::None,
            ]
        );
        // The other stream is unaffected, and a success starts over.
        assert_eq!(follower.failures(StreamKind::System), 0);
        follower.swap_succeeded(StreamKind::Microphone, LAPTOP_MIC);
        assert_eq!(follower.failures(StreamKind::Microphone), 0);
        follower.swap_failed(StreamKind::Microphone);
        assert_eq!(
            failure_notice(follower.failures(StreamKind::Microphone)),
            FailureNotice::SwapFailed
        );
    }

    // --- Headset switched off while its USB dongle stays listed ---------------

    #[test]
    fn listed_but_silent_endpoint_gives_no_os_level_signal() {
        // The reported 14-21 s hole: the dongle stays present and default, so
        // the OS state alone never asks for a switch.
        let (fake, follower) = following_headset();
        // (headset off: Windows still lists the dongle as default and present)
        assert_eq!(follower.evaluate(&fake), Evaluation::default());
    }

    #[test]
    fn silent_default_endpoint_falls_back_to_another_device() {
        let (fake, mut follower) = following_headset();
        follower.mark_dead(StreamKind::Microphone, HEADSET_MIC);

        let eval = follower.evaluate(&fake);
        assert_eq!(
            eval.switches,
            vec![SwitchAction {
                kind: StreamKind::Microphone,
                from: Some(HEADSET_MIC.into()),
                to: LAPTOP_MIC.into(),
                reason: SwitchReason::EndpointSilent,
            }]
        );
        apply(&mut follower, &eval);
        // Settled on the fallback; no ping-pong while the mark stands.
        assert_eq!(follower.evaluate(&fake), Evaluation::default());
    }

    #[test]
    fn cleared_mark_moves_back_to_the_default() {
        let (fake, mut follower) = following_headset();
        follower.mark_dead(StreamKind::Microphone, HEADSET_MIC);
        let eval = follower.evaluate(&fake);
        apply(&mut follower, &eval);

        follower.clear_dead(StreamKind::Microphone);
        let eval = follower.evaluate(&fake);
        assert_eq!(eval.switches.len(), 1);
        assert_eq!(eval.switches[0].to, HEADSET_MIC);
        assert_eq!(eval.switches[0].reason, SwitchReason::DefaultChanged);
    }

    #[test]
    fn silent_endpoint_without_alternative_keeps_the_stream() {
        let fake = FakeDevices::new(&[HEADSET_MIC], HEADSET_MIC, &[SPEAKERS], SPEAKERS);
        let mut follower = DeviceFollower::new(
            true,
            None,
            Some(HEADSET_MIC.into()),
            true,
            None,
            Some(SPEAKERS.into()),
        );
        follower.mark_dead(StreamKind::Microphone, HEADSET_MIC);
        assert_eq!(follower.evaluate(&fake), Evaluation::default());
    }

    #[test]
    fn dead_mark_does_not_override_a_new_working_default() {
        let (fake, mut follower) = following_headset();
        follower.mark_dead(StreamKind::Microphone, HEADSET_MIC);
        let eval = follower.evaluate(&fake);
        apply(&mut follower, &eval);

        // The user picks the Yeti as default while the headset is still off.
        fake.set(
            StreamKind::Microphone,
            &[LAPTOP_MIC, HEADSET_MIC, USB_MIC],
            Some(USB_MIC),
        );
        let eval = follower.evaluate(&fake);
        assert_eq!(eval.switches.len(), 1);
        assert_eq!(eval.switches[0].to, USB_MIC);
    }

    #[test]
    fn dead_endpoint_threshold_is_about_one_second() {
        assert!(!endpoint_is_dead(Duration::from_millis(900)));
        assert!(endpoint_is_dead(Duration::from_secs(1)));
        assert!(endpoint_is_dead(Duration::from_secs(20)));
    }

    #[test]
    fn repairs_double_encoded_danish_device_names() {
        assert_eq!(
            repair_double_utf8("Mikrofon pÃ¥ headset (Jabra Link 390)"),
            "Mikrofon på headset (Jabra Link 390)"
        );
        assert_eq!(repair_double_utf8("HÃ¸jttalere (Senary Audio)"), "Højttalere (Senary Audio)");
        // Uppercase Ø is byte 0x98 in the second position: a cp1252 character.
        assert_eq!(repair_double_utf8("Ã˜ret"), "Øret");
    }

    #[test]
    fn leaves_correct_names_alone() {
        for name in ["Mikrofon på headset", "Højttalere", "Speakers (Realtek(R) Audio)", "マイク"] {
            assert_eq!(repair_double_utf8(name), name);
        }
    }
}
