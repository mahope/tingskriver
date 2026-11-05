//! Windows: which output devices does another process play to right now?
//!
//! For every active render endpoint the audio sessions are listed
//! (`IAudioSessionManager2`). A device counts as playing if it has a session
//! in the active state that belongs to another process than ours and is not
//! the system-sounds session. The result feeds
//! [`RenderSelector`](super::render_follow::RenderSelector).
//!
//! Device names are the endpoint friendly names, the same ones cpal reports,
//! so they can be handed straight to the loopback stream.

use windows::core::Interface;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::{
    eConsole, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
    IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::GetCurrentProcessId;

use super::render_follow::RenderObservation;

/// Keeps COM initialised on this thread for as long as it lives.
struct ComGuard(bool);

impl ComGuard {
    fn new() -> Self {
        // SAFETY: plain COM initialisation of the calling thread. A failure
        // (e.g. the thread is already in another apartment) is not fatal: COM
        // is usable then, and we must not uninitialise what we did not init.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        ComGuard(hr.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful CoInitializeEx in `new`.
            unsafe { CoUninitialize() };
        }
    }
}

fn friendly_name(device: &IMMDevice) -> Result<String, String> {
    // SAFETY: COM calls on interfaces we own; the PROPVARIANT is dropped
    // (and cleared) by its Drop impl.
    unsafe {
        let store = device
            .OpenPropertyStore(STGM_READ)
            .map_err(|e| format!("OpenPropertyStore: {e}"))?;
        let value = store
            .GetValue(&PKEY_Device_FriendlyName)
            .map_err(|e| format!("GetValue(FriendlyName): {e}"))?;
        Ok(value.to_string())
    }
}

/// True if the device has an active session from another process.
fn plays_for_other_process(device: &IMMDevice, own_pid: u32) -> Result<bool, String> {
    // SAFETY: COM calls on interfaces we own.
    unsafe {
        let manager: IAudioSessionManager2 = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("Activate(IAudioSessionManager2): {e}"))?;
        let sessions = manager
            .GetSessionEnumerator()
            .map_err(|e| format!("GetSessionEnumerator: {e}"))?;
        let count = sessions.GetCount().map_err(|e| format!("GetCount: {e}"))?;
        for i in 0..count {
            let Ok(session) = sessions.GetSession(i) else {
                continue;
            };
            if !matches!(session.GetState(), Ok(state) if state == AudioSessionStateActive) {
                continue;
            }
            let Ok(session2) = session.cast::<IAudioSessionControl2>() else {
                continue;
            };
            if session2.IsSystemSoundsSession() == S_OK {
                continue;
            }
            match session2.GetProcessId() {
                Ok(pid) if pid != 0 && pid != own_pid => return Ok(true),
                _ => {}
            }
        }
        Ok(false)
    }
}

/// One poll: the default output device and the devices another process plays
/// to. Blocking; call it from a blocking thread.
pub fn poll() -> Result<RenderObservation, String> {
    let _com = ComGuard::new();
    // SAFETY: COM calls on interfaces we own.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("CoCreateInstance(MMDeviceEnumerator): {e}"))?;
        let own_pid = GetCurrentProcessId();

        let default = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()
            .and_then(|d| friendly_name(&d).ok());

        let devices = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("EnumAudioEndpoints: {e}"))?;
        let count = devices.GetCount().map_err(|e| format!("GetCount: {e}"))?;

        let mut active = Vec::new();
        for i in 0..count {
            let Ok(device) = devices.Item(i) else {
                continue;
            };
            // A device that cannot be inspected is left out: that is the
            // same as "not playing", which keeps the loopback where it is.
            let Ok(name) = friendly_name(&device) else {
                continue;
            };
            if matches!(plays_for_other_process(&device, own_pid), Ok(true)) {
                active.push(name);
            }
        }
        Ok(RenderObservation { default, active })
    }
}
