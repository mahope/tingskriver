//! Windows: keep a recording on the current default devices.
//!
//! When the default microphone or output changes, or the device a stream is
//! bound to disappears (headset unplugged, Bluetooth drops), both the
//! microphone stream and the system-audio loopback stream are reopened on the
//! new device while the recording keeps running. When the headset comes back
//! and becomes the default again, the streams move back.
//!
//! While a stream is down, a silence keeper feeds zeros for it at real-time
//! pace so the mixed track and transcript timestamps keep their timeline.
//!
//! Decisions live in `device_follow` (platform independent, unit tested);
//! this module wires them to IMMNotificationClient, cpal/WASAPI and the
//! recording manager. macOS/Linux keep the upstream device monitor behaviour.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};
use log::{debug, error, info, warn};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::mpsc;

use super::device_follow::{
    describe_switch, silence_samples_due, DeviceFollower, DeviceSnapshot, DeviceSource, StreamKind,
    SwitchAction, SwitchReason,
};
use super::device_monitor::DeviceEvent;
use super::recording_commands::{session_live, RECORDING_MANAGER};
use super::recording_state::{AudioChunk, DeviceType as RecordingDeviceType, RecordingState};
use super::stream::AudioStream;
use super::windows_endpoint_notifier::{EndpointEvent, EndpointNotifier};
use super::{AudioDevice, DeviceType};

/// Wait for the burst of notifications that one plug/unplug produces to
/// settle before acting on it.
const DEBOUNCE: Duration = Duration::from_millis(250);
/// Safety net: re-check devices this often even without notifications.
const HEARTBEAT: Duration = Duration::from_secs(5);
/// Upper bound for opening a stream on the new device.
const STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
/// The pipeline runs at 48 kHz; capture resamples to it.
const PIPELINE_SAMPLE_RATE: u32 = 48_000;
const KEEPER_TICK: Duration = Duration::from_millis(50);

/// Chunk ids for synthetic silence, far away from capture ids and from the
/// pipeline's flush sentinels (>= u64::MAX - 10).
static SILENCE_CHUNK_ID: AtomicU64 = AtomicU64::new(1 << 62);

/// WASAPI view of the devices (what cpal will open).
struct WasapiDevices;

impl DeviceSource for WasapiDevices {
    fn snapshot(&self, kind: StreamKind) -> Result<DeviceSnapshot, String> {
        let host = cpal::host_from_id(cpal::HostId::Wasapi).map_err(|e| e.to_string())?;
        let (default, devices) = match kind {
            StreamKind::Microphone => (
                host.default_input_device(),
                host.input_devices()
                    .map_err(|e| e.to_string())?
                    .collect::<Vec<_>>(),
            ),
            StreamKind::System => (
                host.default_output_device(),
                host.output_devices()
                    .map_err(|e| e.to_string())?
                    .collect::<Vec<_>>(),
            ),
        };
        Ok(DeviceSnapshot {
            default_name: default.and_then(|d| d.name().ok()),
            present: devices.iter().filter_map(|d| d.name().ok()).collect(),
        })
    }
}

fn recording_device_type(kind: StreamKind) -> RecordingDeviceType {
    match kind {
        StreamKind::Microphone => RecordingDeviceType::Microphone,
        StreamKind::System => RecordingDeviceType::System,
    }
}

/// Feeds silence for a stream that is down, at real-time pace.
struct SilenceKeeper {
    stop: Arc<AtomicBool>,
    started: Instant,
}

impl SilenceKeeper {
    fn start(kind: StreamKind, session: Arc<RecordingState>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        let stop_flag = stop.clone();
        tokio::spawn(async move {
            let mut emitted: u64 = 0;
            let mut ticker = tokio::time::interval(KEEPER_TICK);
            loop {
                ticker.tick().await;
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                let due = silence_samples_due(started.elapsed(), emitted, PIPELINE_SAMPLE_RATE);
                if due == 0 {
                    continue;
                }
                let chunk = AudioChunk {
                    data: vec![0.0; due as usize],
                    sample_rate: PIPELINE_SAMPLE_RATE,
                    timestamp: session.get_recording_duration().unwrap_or(0.0),
                    chunk_id: SILENCE_CHUNK_ID.fetch_add(1, Ordering::SeqCst),
                    device_type: recording_device_type(kind),
                };
                if let Err(e) = session.send_audio_chunk(chunk) {
                    debug!("[DEVICE_FOLLOW] Silence keeper for {} stopped: {}", kind, e);
                    break;
                }
                emitted += due;
            }
        });
        Self { stop, started }
    }

    /// Stop feeding silence; returns how long the stream was down.
    fn stop(self) -> Duration {
        self.stop.store(true, Ordering::SeqCst);
        self.started.elapsed()
    }
}

/// Take the stream of `kind` out of the manager (if this session is still
/// current) and stop it outside the lock.
async fn tear_down(kind: StreamKind, session: &Arc<RecordingState>) -> Result<(), String> {
    let old = {
        let mut guard = RECORDING_MANAGER.lock().unwrap();
        let manager = guard.as_mut().ok_or("Recording manager not available")?;
        if !Arc::ptr_eq(manager.get_state(), session) {
            return Err("Session changed".to_string());
        }
        match kind {
            StreamKind::Microphone => manager.take_mic_stream_for_swap(),
            StreamKind::System => manager.take_system_stream_for_swap(),
        }
    };
    if let Some(stream) = old {
        // cpal stop/drop on a removed device may block briefly.
        match tokio::task::spawn_blocking(move || stream.stop()).await {
            Ok(Err(e)) => warn!(
                "[DEVICE_FOLLOW] Stopping old {} stream failed (ignored): {}",
                kind, e
            ),
            Err(e) => warn!(
                "[DEVICE_FOLLOW] Stopping old {} stream panicked (ignored): {}",
                kind, e
            ),
            Ok(Ok(())) => {}
        }
    }
    Ok(())
}

/// Open `kind` on `device_name` and install it in the manager.
async fn open_on(
    kind: StreamKind,
    device_name: &str,
    session: &Arc<RecordingState>,
) -> Result<(), String> {
    let device = Arc::new(AudioDevice::new(
        device_name.to_string(),
        match kind {
            StreamKind::Microphone => DeviceType::Input,
            StreamKind::System => DeviceType::Output,
        },
    ));

    let stream = tokio::time::timeout(
        STREAM_OPEN_TIMEOUT,
        AudioStream::create(
            device.clone(),
            session.clone(),
            recording_device_type(kind),
            None,
        ),
    )
    .await
    .map_err(|_| format!("opening stream timed out after {:?}", STREAM_OPEN_TIMEOUT))?
    .map_err(|e| format!("failed to open stream: {}", e))?;

    let rejected = {
        let mut guard = RECORDING_MANAGER.lock().unwrap();
        match guard.as_mut() {
            Some(manager) if Arc::ptr_eq(manager.get_state(), session) => {
                match kind {
                    StreamKind::Microphone => {
                        let system_name = session.get_system_device().map(|d| d.name.clone());
                        manager.set_mic_stream_after_swap(stream, device, system_name);
                    }
                    StreamKind::System => manager.set_system_stream_after_swap(stream, device),
                }
                None
            }
            _ => Some(stream),
        }
    };
    if let Some(stream) = rejected {
        let _ = tokio::task::spawn_blocking(move || stream.stop()).await;
        return Err("Session changed while opening stream".to_string());
    }
    Ok(())
}

struct Follower<R: Runtime> {
    app: AppHandle<R>,
    session: Arc<RecordingState>,
    follower: DeviceFollower,
    keepers: HashMap<StreamKind, SilenceKeeper>,
}

impl<R: Runtime> Follower<R> {
    async fn evaluate(&mut self) -> Option<Duration> {
        let snapshot_follower = self.follower.clone();
        let eval =
            match tokio::task::spawn_blocking(move || snapshot_follower.evaluate(&WasapiDevices))
                .await
            {
                Ok(eval) => eval,
                Err(e) => {
                    error!("[DEVICE_FOLLOW] Device evaluation panicked: {}", e);
                    return Some(Duration::from_secs(1));
                }
            };

        let mut next: Option<Duration> = if eval.retry {
            Some(Duration::from_millis(500))
        } else {
            None
        };

        for (kind, device) in &eval.lost {
            if self.follower.active(*kind).is_none() {
                continue;
            }
            warn!(
                "[DEVICE_FOLLOW] {} device '{}' is gone and Windows has no replacement yet; filling with silence",
                kind,
                device.as_deref().unwrap_or("<none>")
            );
            let session = self.session.clone();
            self.keepers
                .entry(*kind)
                .or_insert_with(|| SilenceKeeper::start(*kind, session));
            if let Err(e) = tear_down(*kind, &self.session).await {
                debug!("[DEVICE_FOLLOW] Tear-down skipped: {}", e);
                return None;
            }
            self.follower.stream_lost(*kind);
        }

        for action in eval.switches {
            if !session_live(&self.session) {
                return None;
            }
            if let Some(delay) = self.switch(action).await {
                next = Some(next.map_or(delay, |n| n.min(delay)));
            }
        }
        next
    }

    /// Perform one switch. Returns a retry delay if it failed.
    async fn switch(&mut self, action: SwitchAction) -> Option<Duration> {
        let kind = action.kind;
        info!(
            "[DEVICE_FOLLOW] Switching {}: '{}' -> '{}' ({})",
            kind,
            action.from.as_deref().unwrap_or("<none>"),
            action.to,
            action.reason
        );
        let started = Instant::now();
        // Fill the gap with silence from the moment the old stream stops
        // delivering: right away if its device is already gone, otherwise
        // once the still-working old stream has been stopped.
        let old_stream_dead = matches!(
            action.reason,
            SwitchReason::ActiveDisappeared | SwitchReason::StreamMissing
        );
        if old_stream_dead {
            self.ensure_keeper(kind);
        }
        let result = match tear_down(kind, &self.session).await {
            Ok(()) => {
                self.ensure_keeper(kind);
                open_on(kind, &action.to, &self.session).await
            }
            Err(e) => Err(e),
        };

        match result {
            Ok(()) => {
                let gap = self
                    .keepers
                    .remove(&kind)
                    .map(SilenceKeeper::stop)
                    .unwrap_or_else(|| started.elapsed());
                self.follower.swap_succeeded(kind, &action.to);
                // Errors reported by the stream that died must not add up to
                // the "too many errors" stop over a long meeting.
                self.session.clear_recoverable_errors();
                info!("[DEVICE_FOLLOW] {}", describe_switch(&action, gap, Ok(())));
                let event = match kind {
                    StreamKind::Microphone => "mic-device-switched",
                    StreamKind::System => "system-device-switched",
                };
                let _ = self.app.emit(
                    event,
                    serde_json::json!({
                        "device_name": action.to,
                        "previous_device_name": action.from,
                        "reason": action.reason.to_string(),
                        "gap_ms": gap.as_millis() as u64,
                    }),
                );
                None
            }
            Err(e) => {
                let delay = self.follower.swap_failed(kind);
                error!(
                    "[DEVICE_FOLLOW] {}; still filling with silence, retrying in {:?}",
                    describe_switch(&action, started.elapsed(), Err(e.as_str())),
                    delay
                );
                Some(delay)
            }
        }
    }

    fn ensure_keeper(&mut self, kind: StreamKind) {
        let session = self.session.clone();
        self.keepers
            .entry(kind)
            .or_insert_with(|| SilenceKeeper::start(kind, session));
    }

    fn stop_keepers(&mut self) {
        for (kind, keeper) in self.keepers.drain() {
            debug!(
                "[DEVICE_FOLLOW] Stopping silence keeper for {} after {:?}",
                kind,
                keeper.stop()
            );
        }
    }
}

/// Resolve whether the user explicitly picked a device (`Some(name)`) or
/// follows the default (`None`). A requested device that is the current
/// default counts as following the default.
fn preferred_device(requested: Option<&str>, current_default: Option<&str>) -> Option<String> {
    let requested = requested?;
    let name = super::parse_audio_device(requested)
        .map(|d| d.name)
        .unwrap_or_else(|_| requested.to_string());
    if Some(name.as_str()) == current_default {
        None
    } else {
        Some(name)
    }
}

/// Start following the default devices for the recording `session`.
///
/// `requested_mic` / `requested_system`: what the user asked for (`None` =
/// default). `monitor_events`: the upstream polling monitor's events; on
/// Windows they only serve as an extra trigger to re-check devices.
pub fn spawn<R: Runtime>(
    app: AppHandle<R>,
    session: Arc<RecordingState>,
    monitor_events: Option<mpsc::UnboundedReceiver<DeviceEvent>>,
    requested_mic: Option<String>,
    requested_system: Option<String>,
) {
    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel::<EndpointEvent>();
        let notifier = match tokio::task::spawn_blocking(move || EndpointNotifier::start(tx)).await
        {
            Ok(Ok(n)) => Some(n),
            Ok(Err(e)) => {
                warn!(
                    "[DEVICE_FOLLOW] No endpoint notifications ({}); relying on polling",
                    e
                );
                None
            }
            Err(e) => {
                warn!(
                    "[DEVICE_FOLLOW] Notifier start panicked ({}); relying on polling",
                    e
                );
                None
            }
        };

        let defaults = tokio::task::spawn_blocking(|| {
            (
                WasapiDevices
                    .snapshot(StreamKind::Microphone)
                    .ok()
                    .and_then(|s| s.default_name),
                WasapiDevices
                    .snapshot(StreamKind::System)
                    .ok()
                    .and_then(|s| s.default_name),
            )
        })
        .await
        .unwrap_or((None, None));

        let mic_active = session.get_microphone_device().map(|d| d.name.clone());
        let system_active = session.get_system_device().map(|d| d.name.clone());
        let mic_preferred = preferred_device(requested_mic.as_deref(), defaults.0.as_deref());
        let system_preferred = preferred_device(requested_system.as_deref(), defaults.1.as_deref());
        info!(
            "[DEVICE_FOLLOW] Started: mic '{}' ({}), system audio '{}' ({})",
            mic_active.as_deref().unwrap_or("<none>"),
            mic_preferred
                .as_deref()
                .map_or("follows default".to_string(), |p| format!(
                    "pinned to '{}'",
                    p
                )),
            system_active.as_deref().unwrap_or("<none>"),
            system_preferred
                .as_deref()
                .map_or("follows default".to_string(), |p| format!(
                    "pinned to '{}'",
                    p
                )),
        );

        let mut state = Follower {
            app,
            session: session.clone(),
            // A recording without a mic (none available at start) still
            // tracks the mic, so one is picked up when it appears.
            follower: DeviceFollower::new(
                true,
                mic_preferred,
                mic_active,
                true,
                system_preferred,
                system_active,
            ),
            keepers: HashMap::new(),
        };
        let mut monitor_events = monitor_events;
        let mut notifier_alive = notifier.is_some();
        // Check once right away: a system stream that failed to open at start
        // is opened here.
        let mut check_at = Some(tokio::time::Instant::now());
        // We are spawned just before the recording is flagged live.
        let spawned_at = Instant::now();
        let mut seen_live = false;

        loop {
            if session_live(&session) {
                seen_live = true;
            } else if seen_live || spawned_at.elapsed() > Duration::from_secs(10) {
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            let wake = check_at.unwrap_or_else(|| tokio::time::Instant::now() + HEARTBEAT);
            tokio::select! {
                event = rx.recv(), if notifier_alive => {
                    match event {
                        Some(event) => {
                            info!("[DEVICE_FOLLOW] Windows endpoint event: {:?}", event);
                            let at = tokio::time::Instant::now() + DEBOUNCE;
                            check_at = Some(check_at.map_or(at, |c| c.min(at)));
                        }
                        None => {
                            warn!("[DEVICE_FOLLOW] Endpoint notifications ended; relying on polling");
                            notifier_alive = false;
                        }
                    }
                }
                event = async { monitor_events.as_mut().unwrap().recv().await }, if monitor_events.is_some() => {
                    match event {
                        Some(event) => {
                            debug!("[DEVICE_FOLLOW] Device monitor event: {:?}", event);
                            let at = tokio::time::Instant::now() + DEBOUNCE;
                            check_at = Some(check_at.map_or(at, |c| c.min(at)));
                        }
                        None => monitor_events = None,
                    }
                }
                _ = tokio::time::sleep_until(wake) => {
                    check_at = None;
                    if !session_live(&session) {
                        break;
                    }
                    if let Some(delay) = state.evaluate().await {
                        check_at = Some(tokio::time::Instant::now() + delay);
                    }
                }
            }
        }

        state.stop_keepers();
        drop(notifier);
        info!("[DEVICE_FOLLOW] Stopped (recording ended)");
    });
}
