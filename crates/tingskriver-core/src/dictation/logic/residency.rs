//! When a loaded speech model may be unloaded.
//!
//! Models stay loaded after meetings and dictations, so the next call starts
//! without a load (seconds). They are unloaded after a configurable idle time,
//! or earlier when the machine runs out of memory.

use std::time::Duration;

/// Default idle time before a model is unloaded.
pub const DEFAULT_IDLE_MINUTES: u64 = 10;
/// Under memory pressure a model is still kept this long after its last call,
/// so a burst of dictations does not reload it each time.
pub const PRESSURE_GRACE: Duration = Duration::from_secs(30);
/// Below this share of free memory the machine is under pressure.
pub const PRESSURE_FREE_PERCENT: u64 = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// `None`: never unload because of idle time.
    pub idle_timeout: Option<Duration>,
    /// Models that are kept loaded however long they are idle.
    pub keep_warm: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy::from_minutes(DEFAULT_IDLE_MINUTES, Vec::new())
    }
}

impl Policy {
    /// `minutes == 0` turns idle unloading off.
    pub fn from_minutes(minutes: u64, keep_warm: Vec<String>) -> Policy {
        Policy {
            idle_timeout: (minutes > 0).then(|| Duration::from_secs(minutes * 60)),
            keep_warm,
        }
    }

    fn keeps_warm(&self, model: &str) -> bool {
        self.keep_warm.iter().any(|m| m.trim() == model)
    }
}

/// The state of one loaded model at a check.
#[derive(Clone, Debug)]
pub struct Snapshot<'a> {
    pub model: &'a str,
    pub idle_for: Duration,
    /// Calls running right now.
    pub busy: usize,
    /// A meeting is being recorded (its live transcription uses the model).
    pub recording: bool,
    pub memory_pressure: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Idle,
    MemoryPressure,
}

impl Reason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Reason::Idle => "idle",
            Reason::MemoryPressure => "memory pressure",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Keep,
    Unload(Reason),
}

pub fn decide(policy: &Policy, now: &Snapshot) -> Decision {
    if now.busy > 0 || now.recording {
        return Decision::Keep;
    }
    if now.memory_pressure && now.idle_for >= PRESSURE_GRACE {
        return Decision::Unload(Reason::MemoryPressure);
    }
    if policy.keeps_warm(now.model) {
        return Decision::Keep;
    }
    match policy.idle_timeout {
        Some(timeout) if now.idle_for >= timeout => Decision::Unload(Reason::Idle),
        _ => Decision::Keep,
    }
}

/// True when less than [`PRESSURE_FREE_PERCENT`] of memory is available.
pub fn memory_pressure(available_bytes: u64, total_bytes: u64) -> bool {
    total_bytes > 0 && available_bytes.saturating_mul(100) < total_bytes.saturating_mul(PRESSURE_FREE_PERCENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(idle_secs: u64) -> Snapshot<'static> {
        Snapshot {
            model: "parakeet-tdt-0.6b-v3-int8",
            idle_for: Duration::from_secs(idle_secs),
            busy: 0,
            recording: false,
            memory_pressure: false,
        }
    }

    #[test]
    fn stays_loaded_until_idle_timeout() {
        let policy = Policy::default();
        assert_eq!(decide(&policy, &snapshot(0)), Decision::Keep);
        assert_eq!(decide(&policy, &snapshot(9 * 60 + 59)), Decision::Keep);
        assert_eq!(decide(&policy, &snapshot(10 * 60)), Decision::Unload(Reason::Idle));
    }

    #[test]
    fn never_unloads_mid_call_or_during_a_meeting() {
        let policy = Policy::from_minutes(1, Vec::new());
        let busy = Snapshot { busy: 1, ..snapshot(3600) };
        let recording = Snapshot { recording: true, memory_pressure: true, ..snapshot(3600) };
        assert_eq!(decide(&policy, &busy), Decision::Keep);
        assert_eq!(decide(&policy, &recording), Decision::Keep);
    }

    #[test]
    fn keep_warm_and_zero_minutes_disable_idle_unload() {
        let warm = Policy::from_minutes(10, vec!["parakeet-tdt-0.6b-v3-int8".into()]);
        assert_eq!(decide(&warm, &snapshot(24 * 3600)), Decision::Keep);
        let other = Policy::from_minutes(10, vec!["large-v3".into()]);
        assert_eq!(decide(&other, &snapshot(24 * 3600)), Decision::Unload(Reason::Idle));
        let never = Policy::from_minutes(0, Vec::new());
        assert_eq!(never.idle_timeout, None);
        assert_eq!(decide(&never, &snapshot(24 * 3600)), Decision::Keep);
    }

    #[test]
    fn memory_pressure_wins_over_keep_warm_after_grace() {
        let warm = Policy::from_minutes(10, vec!["parakeet-tdt-0.6b-v3-int8".into()]);
        let fresh = Snapshot { memory_pressure: true, ..snapshot(5) };
        let stale = Snapshot { memory_pressure: true, ..snapshot(31) };
        assert_eq!(decide(&warm, &fresh), Decision::Keep);
        assert_eq!(decide(&warm, &stale), Decision::Unload(Reason::MemoryPressure));
    }

    #[test]
    fn pressure_threshold() {
        let gb = 1024 * 1024 * 1024;
        assert!(memory_pressure(gb, 16 * gb));
        assert!(!memory_pressure(2 * gb, 16 * gb));
        assert!(!memory_pressure(0, 0));
    }
}
