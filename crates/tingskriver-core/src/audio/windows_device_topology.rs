//! Windows: read the bus an audio endpoint is connected through.
//!
//! The endpoint's first connector (`IDeviceTopology::GetConnector(0)`) knows
//! which adapter device it is wired to; the adapter's device path starts with
//! the bus enumerator (`usb`, `bthenum`, `hdaudio`, ...), which
//! [`transport_type_from_adapter_path`] turns into a [`TransportType`].
//! Nothing here looks at the device name except to find the endpoint.

use windows::core::PWSTR;
use windows::Win32::Media::Audio::{
    eAll, IDeviceTopology, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};

use super::device_transport::{transport_type_from_adapter_path, TransportType};
use super::windows_render_sessions::{friendly_name, ComGuard};

/// Adapter path an endpoint is connected to.
fn adapter_path(device: &IMMDevice) -> Result<String, String> {
    // SAFETY: COM calls on interfaces we own. The id string is allocated by
    // COM and freed with CoTaskMemFree after it has been copied.
    unsafe {
        let topology: IDeviceTopology = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("Activate(IDeviceTopology): {e}"))?;
        let connector = topology
            .GetConnector(0)
            .map_err(|e| format!("GetConnector(0): {e}"))?;
        let id: PWSTR = connector
            .GetDeviceIdConnectedTo()
            .map_err(|e| format!("GetDeviceIdConnectedTo: {e}"))?;
        let path = id.to_string().map_err(|e| format!("adapter path: {e}"));
        CoTaskMemFree(Some(id.0 as *const _));
        path
    }
}

/// The bus the endpoint called `device_name` (the friendly name cpal reports)
/// is connected through. Blocking COM work; call it from a thread that may
/// block. `Err` says why it could not be read; the caller then falls back to
/// the device name.
pub fn transport_of(device_name: &str) -> Result<TransportType, String> {
    let _com = ComGuard::new();
    // SAFETY: COM calls on interfaces we own.
    let path = unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("CoCreateInstance(MMDeviceEnumerator): {e}"))?;
        let devices = enumerator
            .EnumAudioEndpoints(eAll, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("EnumAudioEndpoints: {e}"))?;
        let count = devices.GetCount().map_err(|e| format!("GetCount: {e}"))?;
        let mut found = None;
        for i in 0..count {
            let Ok(device) = devices.Item(i) else {
                continue;
            };
            if friendly_name(&device).is_ok_and(|n| n == device_name) {
                found = Some(device);
                break;
            }
        }
        let device = found.ok_or_else(|| "no active endpoint with that name".to_string())?;
        adapter_path(&device)?
    };
    transport_type_from_adapter_path(&path).ok_or_else(|| {
        // Only the bus word goes in the message, not the whole path.
        let bus = super::device_transport::enumerator_of(&path).unwrap_or_default();
        format!("unknown bus '{bus}'")
    })
}
