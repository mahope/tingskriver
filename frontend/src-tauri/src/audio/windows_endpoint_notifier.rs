//! Windows: endpoint change notifications via `IMMNotificationClient`.
//!
//! Registers a COM callback on a dedicated MTA thread and forwards every
//! relevant change (default device changed, device added/removed/state
//! changed) as an [`EndpointEvent`]. The callback only sends on a channel: the
//! MMDevice API must not be called back into from inside a notification.

use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;

use log::{debug, error, info, warn};
use tokio::sync::mpsc::UnboundedSender;
use windows::core::{implement, Result as WinResult, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

use super::device_follow::StreamKind;

/// A change reported by Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointEvent {
    /// The console-role default device for `kind` changed (`None`: no default).
    DefaultChanged {
        kind: StreamKind,
        device_id: Option<String>,
    },
    /// A device was added, removed, or changed state (e.g. unplugged).
    DeviceChanged { device_id: String, detail: String },
}

#[implement(IMMNotificationClient)]
struct NotificationClient {
    tx: UnboundedSender<EndpointEvent>,
}

fn pcwstr_to_string(s: &PCWSTR) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: Windows passes a valid NUL-terminated wide string for the
    // duration of the callback; we copy it out immediately.
    unsafe { s.to_string().ok() }
}

impl IMMNotificationClient_Impl for NotificationClient_Impl {
    fn OnDeviceStateChanged(
        &self,
        pwstrdeviceid: &PCWSTR,
        dwnewstate: DEVICE_STATE,
    ) -> WinResult<()> {
        let device_id = pcwstr_to_string(pwstrdeviceid).unwrap_or_default();
        let _ = self.tx.send(EndpointEvent::DeviceChanged {
            device_id,
            detail: format!("state -> {:#x}", dwnewstate.0),
        });
        Ok(())
    }

    fn OnDeviceAdded(&self, pwstrdeviceid: &PCWSTR) -> WinResult<()> {
        let device_id = pcwstr_to_string(pwstrdeviceid).unwrap_or_default();
        let _ = self.tx.send(EndpointEvent::DeviceChanged {
            device_id,
            detail: "added".to_string(),
        });
        Ok(())
    }

    fn OnDeviceRemoved(&self, pwstrdeviceid: &PCWSTR) -> WinResult<()> {
        let device_id = pcwstr_to_string(pwstrdeviceid).unwrap_or_default();
        let _ = self.tx.send(EndpointEvent::DeviceChanged {
            device_id,
            detail: "removed".to_string(),
        });
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        pwstrdefaultdeviceid: &PCWSTR,
    ) -> WinResult<()> {
        // cpal opens the console-role default; the multimedia and
        // communications roles fire separately and would only add noise.
        if role != eConsole {
            return Ok(());
        }
        let kind = if flow == eCapture {
            StreamKind::Microphone
        } else if flow == eRender {
            StreamKind::System
        } else {
            return Ok(());
        };
        let _ = self.tx.send(EndpointEvent::DefaultChanged {
            kind,
            device_id: pcwstr_to_string(pwstrdefaultdeviceid),
        });
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _pwstrdeviceid: &PCWSTR, _key: &PROPERTYKEY) -> WinResult<()> {
        // Fires constantly (volume, format, ...); not relevant for routing.
        Ok(())
    }
}

/// Keeps the notification callback registered until dropped.
pub struct EndpointNotifier {
    stop_tx: Option<std_mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl EndpointNotifier {
    /// Register for endpoint notifications. Events are sent to `tx` until the
    /// returned notifier is dropped.
    pub fn start(tx: UnboundedSender<EndpointEvent>) -> Result<Self, String> {
        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
        let (ready_tx, ready_rx) = std_mpsc::channel::<Result<(), String>>();

        let thread = std::thread::Builder::new()
            .name("audio-endpoint-notifier".to_string())
            .spawn(move || {
                // SAFETY: plain COM initialisation on a thread we own; every
                // successful CoInitializeEx is paired with CoUninitialize.
                let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                if init.is_err() {
                    let _ = ready_tx.send(Err(format!("CoInitializeEx failed: {:?}", init)));
                    return;
                }

                let registered: Result<(IMMDeviceEnumerator, IMMNotificationClient), String> =
                    (|| {
                        // SAFETY: standard MMDevice API calls on an initialised MTA thread.
                        let enumerator: IMMDeviceEnumerator =
                            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                                .map_err(|e| {
                                    format!("CoCreateInstance(MMDeviceEnumerator) failed: {}", e)
                                })?;
                        let client: IMMNotificationClient = NotificationClient { tx }.into();
                        unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }
                            .map_err(|e| {
                                format!("RegisterEndpointNotificationCallback failed: {}", e)
                            })?;
                        Ok((enumerator, client))
                    })();

                match registered {
                    Ok((enumerator, client)) => {
                        let _ = ready_tx.send(Ok(()));
                        // Park until the notifier is dropped (or its sender is).
                        let _ = stop_rx.recv();
                        if let Err(e) =
                            unsafe { enumerator.UnregisterEndpointNotificationCallback(&client) }
                        {
                            warn!(
                                "[DEVICE_FOLLOW] UnregisterEndpointNotificationCallback failed: {}",
                                e
                            );
                        }
                        drop(client);
                        drop(enumerator);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
                unsafe { CoUninitialize() };
                debug!("[DEVICE_FOLLOW] Endpoint notifier thread exited");
            })
            .map_err(|e| format!("Failed to spawn notifier thread: {}", e))?;

        match ready_rx.recv() {
            Ok(Ok(())) => {
                info!("[DEVICE_FOLLOW] Registered IMMNotificationClient for endpoint changes");
                Ok(Self {
                    stop_tx: Some(stop_tx),
                    thread: Some(thread),
                })
            }
            Ok(Err(e)) => {
                let _ = thread.join();
                error!("[DEVICE_FOLLOW] {}", e);
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err("Notifier thread exited before registering".to_string())
            }
        }
    }
}

impl Drop for EndpointNotifier {
    fn drop(&mut self) {
        drop(self.stop_tx.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
