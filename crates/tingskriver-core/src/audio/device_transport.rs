//! Which bus an audio device sits on, read from its adapter path.
//!
//! Windows reports where an endpoint is connected as a device path, for
//! example `{2}.\\?\usb#vid_0b0e&pid_245e&mi_00#...`. The first segment is the
//! bus enumerator (`usb`, `bthenum`, `hdaudio`, ...). That is a fact about the
//! hardware; the device name is only a label the manufacturer chose, and a
//! Jabra headset with a USB dongle is a USB device, not a Bluetooth one.
//!
//! Plain `std` only, so it is unit tested on every platform. The Windows
//! lookup that produces the path is in `windows_device_topology`.

use std::fmt;

/// The bus a device is connected through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    Bluetooth,
    Usb,
    /// On the motherboard or the CPU's audio block (HD Audio, Intel SST, PCI).
    BuiltIn,
    /// Software device: virtual cables, loopback drivers.
    Virtual,
}

impl fmt::Display for TransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TransportType::Bluetooth => "Bluetooth",
            TransportType::Usb => "USB",
            TransportType::BuiltIn => "built-in",
            TransportType::Virtual => "virtual",
        })
    }
}

/// The bus enumerator in an adapter path: `{2}.\\?\usb#vid_...` gives `usb`.
/// Lower case; `None` if the path has no recognisable first segment.
pub fn enumerator_of(adapter_path: &str) -> Option<String> {
    let mut rest = adapter_path.trim();
    // "{2}." (or "{1}.") is the data-flow prefix Windows puts in front.
    if rest.starts_with('{') {
        rest = rest.split_once("}.").map(|(_, r)| r)?;
    }
    let rest = rest.trim_start_matches('\\').trim_start_matches('?');
    let rest = rest.trim_start_matches('\\');
    let segment = rest.split(['#', '\\']).next()?.trim();
    if segment.is_empty() {
        None
    } else {
        Some(segment.to_ascii_lowercase())
    }
}

/// Transport type from a bus enumerator name. `None` for a bus we do not
/// know; the caller then falls back to the device name.
pub fn transport_type_from_enumerator(enumerator: &str) -> Option<TransportType> {
    let e = enumerator.to_ascii_lowercase();
    if e.starts_with("bth") {
        // BTHENUM (A2DP), BTHHFENUM (hands-free), BTHLEENUM (LE audio),
        // BTHPORT.
        Some(TransportType::Bluetooth)
    } else if e.starts_with("usb") {
        Some(TransportType::Usb)
    } else if e == "hdaudio" || e == "intelaudio" || e == "pci" || e == "acpi" {
        Some(TransportType::BuiltIn)
    } else if e == "swd" || e == "root" {
        Some(TransportType::Virtual)
    } else {
        None
    }
}

/// Transport type from a full adapter path.
pub fn transport_type_from_adapter_path(adapter_path: &str) -> Option<TransportType> {
    transport_type_from_enumerator(&enumerator_of(adapter_path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Adapter paths in the shape Windows reports them. The instance ids are
    // made up; only the enumerator and path layout matter.
    const BTHHFENUM: &str = r"{2}.\\?\bthhfenum#bthenum_hfpaghf#8&1f2e3d4c&0&bthhfenum#{6994ad04-93ef-11d0-a3cc-00a0c9223196}\hfpaghf";
    const BTHENUM: &str = r"{2}.\\?\bthenum#{0000110b-0000-1000-8000-00805f9b34fb}_vid&0001004c_pid&200e#7&2a3b4c5d&0&112233445566_c00000000#{6994ad04-93ef-11d0-a3cc-00a0c9223196}\a2dpsnk";
    const USB: &str = r"{2}.\\?\usb#vid_0b0e&pid_245e&mi_00#6&2d6f1e8b&0&0000#{6994ad04-93ef-11d0-a3cc-00a0c9223196}\global";
    const HDAUDIO: &str = r"{2}.\\?\hdaudio#func_01&ven_10ec&dev_0256&subsys_17aa22bf&rev_1000#5&22f1c3d&0&0001#{6994ad04-93ef-11d0-a3cc-00a0c9223196}\elnrender";
    const VIRTUAL: &str =
        r"{2}.\\?\root#media#0000#{6994ad04-93ef-11d0-a3cc-00a0c9223196}\vb-audio-cable";
    const SWD: &str = r"{2}.\\?\swd#mmdevapi#{0.0.0.00000000}.{5a1b2c3d-0000-0000-0000-000000000001}#{e6327cad-dcec-4949-ae8a-991e976a79d2}";

    #[test]
    fn five_real_adapter_paths_give_the_right_type() {
        assert_eq!(
            transport_type_from_adapter_path(BTHHFENUM),
            Some(TransportType::Bluetooth)
        );
        assert_eq!(
            transport_type_from_adapter_path(BTHENUM),
            Some(TransportType::Bluetooth)
        );
        assert_eq!(
            transport_type_from_adapter_path(USB),
            Some(TransportType::Usb)
        );
        assert_eq!(
            transport_type_from_adapter_path(HDAUDIO),
            Some(TransportType::BuiltIn)
        );
        assert_eq!(
            transport_type_from_adapter_path(VIRTUAL),
            Some(TransportType::Virtual)
        );
    }

    #[test]
    fn software_device_enumerator_is_virtual() {
        assert_eq!(
            transport_type_from_adapter_path(SWD),
            Some(TransportType::Virtual)
        );
    }

    #[test]
    fn enumerators_are_extracted_in_lower_case() {
        assert_eq!(enumerator_of(USB).as_deref(), Some("usb"));
        assert_eq!(enumerator_of(BTHHFENUM).as_deref(), Some("bthhfenum"));
        assert_eq!(
            enumerator_of(r"{2}.\\?\HDAUDIO#FUNC_01#x").as_deref(),
            Some("hdaudio")
        );
        // Without the {n}. prefix and without \\?\.
        assert_eq!(
            enumerator_of(r"USB\VID_0B0E&PID_245E\6&2D").as_deref(),
            Some("usb")
        );
    }

    #[test]
    fn intel_smart_sound_and_pci_are_built_in() {
        assert_eq!(
            transport_type_from_enumerator("intelaudio"),
            Some(TransportType::BuiltIn)
        );
        assert_eq!(
            transport_type_from_enumerator("PCI"),
            Some(TransportType::BuiltIn)
        );
    }

    #[test]
    fn unknown_bus_or_garbage_gives_none() {
        assert_eq!(transport_type_from_adapter_path(r"{2}.\\?\xyzzy#a#b"), None);
        assert_eq!(transport_type_from_adapter_path(""), None);
        assert_eq!(transport_type_from_adapter_path("{2}."), None);
        assert_eq!(transport_type_from_adapter_path("{2}"), None);
    }

    #[test]
    fn a_bus_name_that_merely_contains_the_letters_is_not_matched() {
        // "xbth" is not a Bluetooth enumerator; only a prefix counts.
        assert_eq!(transport_type_from_enumerator("xbthenum"), None);
        assert_eq!(transport_type_from_enumerator("husb"), None);
    }
}
