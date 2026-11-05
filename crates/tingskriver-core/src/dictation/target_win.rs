//! The window and text field dictation writes into (Windows): what modes
//! match on, and the keys for "slet det", "vælg det" and "tryk Enter".
//!
//! Nothing here reads what the user types; it only looks at which window
//! and control has focus, and sends a fixed number of keys.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_BACK, VK_LEFT, VK_RETURN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowTextW, GetWindowThreadProcessId,
    GUITHREADINFO,
};

use super::logic::modes::ForegroundWindow;
use super::logic::spoken::{EditPlan, FieldId};
use super::logic::target_check::{ElementProps, FieldProbe, Focus};

fn text_of(read: impl FnOnce(&mut [u16]) -> i32) -> Option<String> {
    let mut buf = [0u16; 512];
    let len = read(&mut buf);
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

/// Program, window class and title of the foreground window.
pub fn foreground_window() -> ForegroundWindow {
    let process = super::win::foreground_process();
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return ForegroundWindow {
                process,
                ..ForegroundWindow::default()
            };
        }
        ForegroundWindow {
            process,
            class: text_of(|buf| GetClassNameW(hwnd, buf)),
            title: text_of(|buf| GetWindowTextW(hwnd, buf)),
        }
    }
}

/// What UI Automation says about an element. A failed call is `None`
/// (not reported), which never blocks dictation.
struct UiaElement<'a>(&'a windows::Win32::UI::Accessibility::IUIAutomationElement);

impl ElementProps for UiaElement<'_> {
    fn is_password(&self) -> Option<bool> {
        unsafe { self.0.CurrentIsPassword().ok().map(|b| b.as_bool()) }
    }

    fn is_enabled(&self) -> Option<bool> {
        unsafe { self.0.CurrentIsEnabled().ok().map(|b| b.as_bool()) }
    }

    fn is_read_only(&self) -> Option<bool> {
        use windows::core::Interface;
        use windows::Win32::UI::Accessibility::{IUIAutomationValuePattern, UIA_ValuePatternId};
        unsafe {
            // No Value pattern (many browser and Electron fields): not reported.
            let pattern = self.0.GetCurrentPattern(UIA_ValuePatternId).ok()?;
            let value: IUIAutomationValuePattern = pattern.cast().ok()?;
            value.CurrentIsReadOnly().ok().map(|b| b.as_bool())
        }
    }
}

/// UI Automation runtime id and properties of the focused element. Browsers
/// and Electron apps draw all their text fields in one control; the id tells
/// them apart.
fn focused_element() -> (Option<Vec<i32>>, FieldProbe) {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Ole::{
        SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
        SafeArrayUnaccessData,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

    unsafe {
        // A successful CoInitializeEx is paired with CoUninitialize; on a
        // thread that already has COM, the existing apartment is used.
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mut probe = FieldProbe::default();
        let id = (|| {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let element = automation.GetFocusedElement().ok()?;
            probe = FieldProbe::read(&UiaElement(&element));
            let array = element.GetRuntimeId().ok()?;
            if array.is_null() {
                return None;
            }
            let read = (|| {
                let lower = SafeArrayGetLBound(array, 1).ok()?;
                let upper = SafeArrayGetUBound(array, 1).ok()?;
                let len = usize::try_from(upper - lower + 1).ok()?;
                let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
                SafeArrayAccessData(array, &mut data).ok()?;
                let id = if data.is_null() {
                    None
                } else {
                    Some(std::slice::from_raw_parts(data as *const i32, len).to_vec())
                };
                let _ = SafeArrayUnaccessData(array);
                id
            })();
            let _ = SafeArrayDestroy(array);
            read.filter(|id| !id.is_empty())
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        (id, probe)
    }
}

/// The control with keyboard focus in the foreground window, and the UI
/// Automation element inside it.
pub fn focused_field() -> Option<FieldId> {
    focus().map(|focus| focus.id)
}

/// The focused field with the properties that decide whether dictation may
/// write into it ([`super::logic::target_check`]).
pub fn focus() -> Option<Focus> {
    let process = super::win::foreground_process()?;
    unsafe {
        let window: HWND = GetForegroundWindow();
        if window.0.is_null() {
            return None;
        }
        let thread = GetWindowThreadProcessId(window, None);
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let control = match GetGUIThreadInfo(thread, &mut info) {
            Ok(()) if !info.hwndFocus.0.is_null() => info.hwndFocus,
            _ => window,
        };
        let (element, probe) = focused_element();
        Some(Focus {
            id: FieldId {
                process,
                window: window.0 as isize,
                control: control.0 as isize,
                element,
            },
            probe,
        })
    }
}

fn key(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn tap(inputs: &mut Vec<INPUT>, vk: VIRTUAL_KEY) {
    inputs.push(key(vk, KEYBD_EVENT_FLAGS(0)));
    inputs.push(key(vk, KEYEVENTF_KEYUP));
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    for chunk in inputs.chunks(64) {
        let sent = unsafe { SendInput(chunk, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != chunk.len() {
            return Err(
                "Tasterne blev blokeret af programmet (kører det som administrator?)".to_string(),
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    Ok(())
}

/// Send the keys for "slet det" or "vælg det".
pub fn apply_edit(plan: EditPlan) -> Result<(), String> {
    let mut inputs = Vec::new();
    match plan {
        EditPlan::Backspace(n) => (0..n).for_each(|_| tap(&mut inputs, VK_BACK)),
        EditPlan::SelectLeft(n) => {
            inputs.push(key(VK_SHIFT, KEYBD_EVENT_FLAGS(0)));
            (0..n).for_each(|_| tap(&mut inputs, VK_LEFT));
            inputs.push(key(VK_SHIFT, KEYEVENTF_KEYUP));
        }
    }
    send(&inputs)
}

/// Press Enter (chat modes, after the trigger word).
pub fn press_enter() -> Result<(), String> {
    let mut inputs = Vec::new();
    tap(&mut inputs, VK_RETURN);
    send(&inputs)
}
