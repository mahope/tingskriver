//! Scripted device information and microphone callbacks on a caller-owned clock.
//! No audio backend, sleeping, or wall-clock reads are involved. Each buffer event
//! represents one callback; long recordings advance the same virtual clock.
//!
//! ```
//! use std::time::Duration;
//! use tingskriver_core::{audio::device_follow::MicSignal, fake_device::*};
//! let mut device = FakeDevice::new(vec![TimedEvent {
//!     at: Duration::from_secs(1), event: DeviceEvent::Buffer(Buffer::Zeros),
//! }])?;
//! device.advance_to(Duration::from_secs(1))?;
//! assert_eq!(device.since_frame(), Some(Duration::ZERO));
//! # Ok::<(), String>(())
//! ```

use crate::audio::device_follow::{DeviceSnapshot, DeviceSource, MicSignal, StreamKind};
use std::cell::Cell;
use std::collections::HashMap;
use std::time::Duration;

/// Buffer contents, deliberately distinct from absence of a callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Buffer {
    Sound,
    Silence,
    Zeros,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    Buffer(Buffer),
    /// Replace an OS snapshot, including removal, return, or default changes.
    Devices(StreamKind, DeviceSnapshot),
    /// Delivered to the caller, which forwards it to its stream error handler.
    StreamError(StreamKind, String),
    /// Make enumeration fail until a new snapshot is supplied for this kind.
    SnapshotError(StreamKind, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedEvent {
    pub at: Duration,
    pub event: DeviceEvent,
}

/// One microphone signal plus snapshots for both capture kinds.
/// Events at equal timestamps retain script order. Missing buffers are modeled
/// by advancing time without a buffer event. Errors are returned exactly once
/// from `advance_to`; snapshots persist until replaced. A new stream must call
/// `MicSignal::reset`, just as the real watchdog does after a successful swap.
pub struct FakeDevice {
    script: Vec<TimedEvent>,
    next: usize,
    now: Duration,
    last_frame: Cell<Option<Duration>>,
    last_sound: Cell<Option<Duration>>,
    snapshots: HashMap<StreamKind, Result<DeviceSnapshot, String>>,
}

impl FakeDevice {
    /// Reject unordered scripts rather than silently changing their meaning.
    pub fn new(script: Vec<TimedEvent>) -> Result<Self, String> {
        if script.windows(2).any(|w| w[0].at > w[1].at) {
            return Err("fake device events must be ordered by virtual time".into());
        }
        Ok(Self {
            script,
            next: 0,
            now: Duration::ZERO,
            last_frame: Cell::new(None),
            last_sound: Cell::new(None),
            snapshots: HashMap::new(),
        })
    }

    /// Apply all events through `now`, returning stream errors for the caller.
    /// Backwards time fails without changing state.
    pub fn advance_to(&mut self, now: Duration) -> Result<Vec<(StreamKind, String)>, String> {
        if now < self.now {
            return Err("fake device clock cannot move backwards".into());
        }
        let mut errors = Vec::new();
        while let Some(timed) = self.script.get(self.next).filter(|e| e.at <= now) {
            match &timed.event {
                DeviceEvent::Buffer(buffer) => {
                    self.last_frame.set(Some(timed.at));
                    // A silent first callback establishes the start of silence.
                    if *buffer == Buffer::Sound || self.last_sound.get().is_none() {
                        self.last_sound.set(Some(timed.at));
                    }
                }
                DeviceEvent::Devices(kind, snapshot) => {
                    self.snapshots.insert(*kind, Ok(snapshot.clone()));
                }
                DeviceEvent::SnapshotError(kind, error) => {
                    self.snapshots.insert(*kind, Err(error.clone()));
                }
                DeviceEvent::StreamError(kind, device) => errors.push((*kind, device.clone())),
            }
            self.next += 1;
        }
        self.now = now;
        Ok(errors)
    }
}

impl DeviceSource for FakeDevice {
    fn snapshot(&self, kind: StreamKind) -> Result<DeviceSnapshot, String> {
        self.snapshots
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| Ok(DeviceSnapshot::default()))
    }
}

impl MicSignal for FakeDevice {
    fn since_frame(&self) -> Option<Duration> {
        self.last_frame.get().map(|at| self.now - at)
    }

    fn since_sound(&self) -> Option<Duration> {
        self.last_sound.get().map(|at| self.now - at)
    }

    fn reset(&self) {
        self.last_frame.set(None);
        self.last_sound.set(None);
    }
}

#[cfg(test)]
mod tests;
