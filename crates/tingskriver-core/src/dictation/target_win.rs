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

/// The control with keyboard focus in the foreground window.
///
/// Browsers and Electron apps draw all their text fields in one control, so
/// there "the same field" means the same page or app window.
pub fn focused_field() -> Option<FieldId> {
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
        Some(FieldId {
            process,
            window: window.0 as isize,
            control: control.0 as isize,
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
