//! Windows' power and session messages as [`PowerEvent`]s (issue #179).
//!
//! The host window gets `WM_POWERBROADCAST` (through
//! `RegisterSuspendResumeNotification`), `WM_WTSSESSION_CHANGE` (lock and
//! unlock) and, on a hidden top-level window, `WM_QUERYENDSESSION` and
//! `WM_ENDSESSION`. [`event_from_message`] turns the raw numbers into events
//! and is plain code, so it is tested on Linux. [`PowerSource`] is what
//! produces events: `win::HostThread` on Windows, [`FakePowerSource`] in
//! tests. A sink runs on the thread that got the message and Windows waits
//! for it, so it must save the recording before it returns.

use std::sync::{Arc, Mutex};

use tingskriver_core::audio::power::PowerEvent;

// The same numbers as windows-sys, so this builds on every system.
pub const WM_QUERYENDSESSION: u32 = 0x0011;
pub const WM_ENDSESSION: u32 = 0x0016;
pub const WM_POWERBROADCAST: u32 = 0x0218;
pub const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
pub const PBT_APMSUSPEND: usize = 4;
pub const PBT_APMRESUMEAUTOMATIC: usize = 18;
pub const PBT_APMRESUMESUSPEND: usize = 7;
pub const WTS_SESSION_LOCK: usize = 7;
pub const WTS_SESSION_UNLOCK: usize = 8;

/// What the host does with an event.
pub type PowerSink = Arc<dyn Fn(PowerEvent) + Send + Sync>;

/// The event a window message means, if it is one we handle. Both resume
/// messages announce: `PBT_APMRESUMESUSPEND` (resume after a user action)
/// can come alone, and after `PBT_APMRESUMEAUTOMATIC` it finds nothing
/// pending, so the user is told once.
pub fn event_from_message(msg: u32, wparam: usize) -> Option<PowerEvent> {
    match (msg, wparam) {
        (WM_POWERBROADCAST, PBT_APMSUSPEND) => Some(PowerEvent::Suspend),
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) => {
            Some(PowerEvent::Resume)
        }
        (WM_QUERYENDSESSION, _) => Some(PowerEvent::QueryEndSession),
        (WM_ENDSESSION, ending) => Some(PowerEvent::EndSession {
            ending: ending != 0,
        }),
        (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK) => Some(PowerEvent::Lock),
        (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK) => Some(PowerEvent::Unlock),
        _ => None,
    }
}

/// Something that delivers [`PowerEvent`]s to a sink.
pub trait PowerSource {
    fn subscribe(&self, sink: PowerSink);
}

/// Fires events by hand: the stand-in for Windows in tests.
#[derive(Default)]
pub struct FakePowerSource {
    sink: Mutex<Option<PowerSink>>,
}

impl FakePowerSource {
    /// Deliver `event` on the calling thread and return when the sink does,
    /// as Windows does for a window message.
    pub fn fire(&self, event: PowerEvent) {
        let sink = self.sink.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(sink) = sink {
            sink(event);
        }
    }
}

impl PowerSource for FakePowerSource {
    fn subscribe(&self, sink: PowerSink) {
        *self.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use tingskriver_core::audio::power::{
        InterruptCause, InterruptNotice, PowerController, PowerSession,
    };

    #[test]
    fn windows_messages_become_events() {
        assert_eq!(
            event_from_message(WM_POWERBROADCAST, PBT_APMSUSPEND),
            Some(PowerEvent::Suspend)
        );
        assert_eq!(
            event_from_message(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC),
            Some(PowerEvent::Resume)
        );
        // A wake-up by the user can come without the automatic one.
        assert_eq!(
            event_from_message(WM_POWERBROADCAST, PBT_APMRESUMESUSPEND),
            Some(PowerEvent::Resume)
        );
        assert_eq!(
            event_from_message(WM_QUERYENDSESSION, 0),
            Some(PowerEvent::QueryEndSession)
        );
        assert_eq!(
            event_from_message(WM_ENDSESSION, 1),
            Some(PowerEvent::EndSession { ending: true })
        );
        assert_eq!(
            event_from_message(WM_ENDSESSION, 0),
            Some(PowerEvent::EndSession { ending: false })
        );
        assert_eq!(
            event_from_message(WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK),
            Some(PowerEvent::Lock)
        );
        assert_eq!(
            event_from_message(WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK),
            Some(PowerEvent::Unlock)
        );
        // Battery status and other power messages are not ours.
        assert_eq!(event_from_message(WM_POWERBROADCAST, 10), None);
        assert_eq!(event_from_message(WM_WTSSESSION_CHANGE, 5), None);
        assert_eq!(event_from_message(0x0001, 0), None);
    }

    /// A recording that notes in a shared log when its audio is safe.
    struct Disk {
        log: Arc<Mutex<Vec<&'static str>>>,
        recording: Mutex<bool>,
    }

    impl PowerSession for Disk {
        fn recording_active(&self) -> bool {
            *self.recording.lock().unwrap()
        }
        fn finish_recording(&self, cause: InterruptCause, at: &str) -> Option<InterruptNotice> {
            self.log.lock().unwrap().push("audio on disk");
            *self.recording.lock().unwrap() = false;
            Some(InterruptNotice {
                cause,
                at: at.into(),
                meeting_folder: None,
                audio_status: Some("ok".into()),
                message: cause.message().into(),
            })
        }
        fn save_database(&self, _budget: std::time::Duration) {
            self.log.lock().unwrap().push("database saved");
        }
        fn announce(&self, _: &InterruptNotice) {
            self.log.lock().unwrap().push("user told");
        }
    }

    #[test]
    fn the_machine_sleeps_only_after_the_recording_is_saved() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let controller = Arc::new(PowerController::new(Disk {
            log: log.clone(),
            recording: Mutex::new(true),
        }));
        let source = FakePowerSource::default();
        {
            let controller = controller.clone();
            source.subscribe(Arc::new(move |event| controller.handle(event)));
        }
        source.fire(PowerEvent::Suspend);
        // `fire` is what Windows waits for: when it returns the machine may sleep.
        log.lock().unwrap().push("machine sleeps");
        source.fire(PowerEvent::Resume);
        assert_eq!(
            *log.lock().unwrap(),
            [
                "audio on disk",
                "database saved",
                "machine sleeps",
                "user told"
            ]
        );
    }
}
