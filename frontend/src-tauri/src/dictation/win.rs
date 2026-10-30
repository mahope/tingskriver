//! Windows-only pieces of dictation: the global keyboard hook, text insertion
//! via the clipboard or `SendInput`, the foreground program, Credential
//! Manager and overlay window styles.
//!
//! Depends only on `std`, `log`, the `windows` crate and `logic`, so it can be
//! type-checked for `x86_64-pc-windows-msvc` without the rest of the app.

use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, GlobalFree, ERROR_NOT_FOUND, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, RECT,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
    GetClipboardSequenceNumber,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RETURN, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowThreadProcessId,
    PeekMessageW, PostThreadMessageW, SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW, ShowWindow,
    UnhookWindowsHookEx, GWL_EXSTYLE, HC_ACTION, HWND_TOPMOST, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

use super::logic::hotkey::{Chord, ChordSignal, ChordTracker};

/// Marks input we inject ourselves, so the hook lets it straight through.
const INJECT_MARKER: usize = 0x5453_4B52; // "TSKR"
/// Unassigned virtual key used to stop Win key-up from opening Start.
const VK_MASK: u16 = 0xE8;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// Keyboard hook
// ---------------------------------------------------------------------------

struct HookShared {
    tracker: ChordTracker,
    sender: Sender<(ChordSignal, Instant)>,
}

static HOOK_STATE: Mutex<Option<HookShared>> = Mutex::new(None);

/// A running `WH_KEYBOARD_LL` hook. Dropping it removes the hook.
pub struct KeyboardHook {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl KeyboardHook {
    /// Install the hook on its own thread with a message loop.
    pub fn start(chord: Chord, sender: Sender<(ChordSignal, Instant)>) -> Result<KeyboardHook, String> {
        *HOOK_STATE.lock().map_err(|_| "hook state poisoned")? = Some(HookShared {
            tracker: ChordTracker::new(chord),
            sender,
        });

        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32, String>>();
        let thread = std::thread::Builder::new()
            .name("dictation-keyboard-hook".into())
            .spawn(move || unsafe {
                let mut msg = MSG::default();
                // Make sure this thread has a message queue before reporting its id.
                let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                let module = match GetModuleHandleW(None) {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("GetModuleHandleW: {e}")));
                        return;
                    }
                };
                let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(module.into()), 0) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("SetWindowsHookExW: {e}")));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                while GetMessageW(&mut msg, None, 0, 0).0 > 0 {}
                let _ = UnhookWindowsHookEx(hook);
            })
            .map_err(|e| format!("could not start hook thread: {e}"))?;

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(thread_id)) => Ok(KeyboardHook {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("keyboard hook did not start".to_string()),
        }
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Ok(mut state) = HOOK_STATE.lock() {
            *state = None;
        }
    }
}

/// Runs for every key press in the system. Must return fast and never record keys.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if kb.dwExtraInfo != INJECT_MARKER {
            let message = wparam.0 as u32;
            let is_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            let is_up = message == WM_KEYUP || message == WM_SYSKEYUP;
            if is_down || is_up {
                let decision = match HOOK_STATE.try_lock() {
                    Ok(mut guard) => guard.as_mut().map(|state| {
                        let decision = if is_down {
                            if let Some(signal) = state.tracker.resync(|vk| key_is_down(VIRTUAL_KEY(vk as u16))) {
                                let _ = state.sender.send((signal, Instant::now()));
                            }
                            state.tracker.key_down(kb.vkCode)
                        } else {
                            state.tracker.key_up(kb.vkCode)
                        };
                        if let Some(signal) = decision.signal {
                            let _ = state.sender.send((signal, Instant::now()));
                        }
                        decision
                    }),
                    Err(_) => None,
                };
                if let Some(decision) = decision {
                    if let Some(win_vk) = decision.inject_win_release {
                        send_masked_win_release(win_vk as u16);
                    }
                    if decision.swallow {
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECT_MARKER,
            },
        },
    }
}

/// Replace a swallowed Win key-up with mask key + Win key-up, so Windows
/// sees the key released but does not open the Start menu.
fn send_masked_win_release(win_vk: u16) {
    let inputs = [
        key_input(VK_MASK, 0, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_MASK, 0, KEYEVENTF_KEYUP),
        key_input(win_vk, 0, KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

// ---------------------------------------------------------------------------
// Foreground program
// ---------------------------------------------------------------------------

/// File name of the program owning the foreground window, e.g. "Code.exe".
pub fn foreground_process() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(str::to_string)
    }
}

// ---------------------------------------------------------------------------
// Text insertion
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertMethod {
    Paste,
    Typed,
}

impl InsertMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            InsertMethod::Paste => "paste",
            InsertMethod::Typed => "typed",
        }
    }
}

/// Formats that hold GDI handles rather than global memory; not copied.
const GDI_FORMATS: &[u32] = &[2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];
const MAX_SAVED_BYTES: usize = 32 * 1024 * 1024;
/// OLE bookkeeping formats hold pointers into the copying process; copying
/// their bytes back would hand other programs dangling data.
const SKIPPED_FORMAT_NAMES: &[&str] = &["DataObject", "Ole Private Data", "OwnerLink", "ObjectLink", "Link"];

fn is_skipped_format(format: u32) -> bool {
    if GDI_FORMATS.contains(&format) {
        return true;
    }
    if format < 0xC000 {
        return false;
    }
    let mut buf = [0u16; 128];
    let len = unsafe { GetClipboardFormatNameW(format, &mut buf) };
    if len <= 0 {
        return false;
    }
    let name = String::from_utf16_lossy(&buf[..len as usize]);
    SKIPPED_FORMAT_NAMES.contains(&name.as_str())
}

struct SavedClipboard {
    formats: Vec<(u32, Vec<u8>)>,
}

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<ClipboardGuard, String> {
        // Another program may hold the clipboard for a moment.
        for _ in 0..20 {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(ClipboardGuard);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err("could not open clipboard".to_string())
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn save_clipboard() -> Result<SavedClipboard, String> {
    let _guard = ClipboardGuard::open()?;
    let mut formats = Vec::new();
    let mut total = 0usize;
    unsafe {
        let mut format = EnumClipboardFormats(0);
        while format != 0 {
            if !is_skipped_format(format) {
                if let Ok(handle) = GetClipboardData(format) {
                    let global = HGLOBAL(handle.0);
                    let size = GlobalSize(global);
                    if size > 0 && total + size <= MAX_SAVED_BYTES {
                        let ptr = GlobalLock(global) as *const u8;
                        if !ptr.is_null() {
                            formats.push((format, std::slice::from_raw_parts(ptr, size).to_vec()));
                            total += size;
                            let _ = GlobalUnlock(global);
                        }
                    }
                }
            }
            format = EnumClipboardFormats(format);
        }
    }
    Ok(SavedClipboard { formats })
}

unsafe fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
    let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|e| format!("GlobalAlloc: {e}"))?;
    let ptr = GlobalLock(global) as *mut u8;
    if ptr.is_null() {
        let _ = GlobalFree(Some(global));
        return Err("GlobalLock failed".to_string());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    let _ = GlobalUnlock(global);
    if let Err(e) = SetClipboardData(format, Some(HANDLE(global.0))) {
        let _ = GlobalFree(Some(global));
        return Err(format!("SetClipboardData: {e}"));
    }
    Ok(())
}

fn registered_format(name: &str) -> u32 {
    let name = wide(name);
    unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
}

/// Put `text` on the clipboard, marked so clipboard history and cloud sync skip it.
fn set_clipboard_text(text: &str) -> Result<u32, String> {
    let guard = ClipboardGuard::open()?;
    unsafe {
        EmptyClipboard().map_err(|e| format!("EmptyClipboard: {e}"))?;
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::slice::from_raw_parts(utf16.as_ptr() as *const u8, utf16.len() * 2);
        set_clipboard_bytes(CF_UNICODETEXT.0 as u32, bytes)?;
        let zero = 0u32.to_le_bytes();
        for name in [
            "ExcludeClipboardContentFromMonitorProcessing",
            "CanIncludeInClipboardHistory",
            "CanUploadToCloudClipboard",
        ] {
            let format = registered_format(name);
            if format != 0 {
                let _ = set_clipboard_bytes(format, &zero);
            }
        }
    }
    // Read the number only after CloseClipboard, which may bump it.
    drop(guard);
    Ok(unsafe { GetClipboardSequenceNumber() })
}

fn restore_clipboard(saved: &SavedClipboard) -> Result<(), String> {
    let _guard = ClipboardGuard::open()?;
    unsafe {
        EmptyClipboard().map_err(|e| format!("EmptyClipboard: {e}"))?;
        for (format, bytes) in &saved.formats {
            if let Err(e) = set_clipboard_bytes(*format, bytes) {
                log::debug!("Dictation: could not restore clipboard format {format}: {e}");
            }
        }
    }
    Ok(())
}

fn key_is_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 }
}

/// Wait (max `timeout`) until Win, Alt and Shift are up, so Ctrl+V is not
/// turned into another shortcut by keys the user is still holding.
fn wait_for_modifiers_released(timeout: Duration) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if ![VK_LWIN, VK_RWIN, VK_MENU, VK_SHIFT].iter().any(|vk| key_is_down(*vk)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn send_inputs(inputs: &[INPUT]) -> bool {
    if inputs.is_empty() {
        return true;
    }
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

fn send_ctrl_v() -> bool {
    let ctrl_held = key_is_down(VK_CONTROL);
    let v = 'V' as u16;
    let mut inputs = Vec::with_capacity(4);
    if !ctrl_held {
        inputs.push(key_input(VK_CONTROL.0, 0, KEYBD_EVENT_FLAGS(0)));
    }
    inputs.push(key_input(v, 0, KEYBD_EVENT_FLAGS(0)));
    inputs.push(key_input(v, 0, KEYEVENTF_KEYUP));
    if !ctrl_held {
        inputs.push(key_input(VK_CONTROL.0, 0, KEYEVENTF_KEYUP));
    }
    send_inputs(&inputs)
}

/// Type `text` as Unicode key events (for fields that block paste).
pub fn type_text(text: &str) -> bool {
    let mut inputs = Vec::new();
    for unit in text.encode_utf16() {
        match unit {
            0x0D => {}
            0x0A => {
                inputs.push(key_input(VK_RETURN.0, 0, KEYBD_EVENT_FLAGS(0)));
                inputs.push(key_input(VK_RETURN.0, 0, KEYEVENTF_KEYUP));
            }
            _ => {
                inputs.push(key_input(0, unit, KEYEVENTF_UNICODE));
                inputs.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
        }
    }
    inputs.chunks(64).all(|chunk| {
        let ok = send_inputs(chunk);
        std::thread::sleep(Duration::from_millis(2));
        ok
    })
}

/// Insert `text` in the focused program. Blocks for about `restore_after`.
/// Returns how the text was inserted and when it reached the program.
///
/// The clipboard is saved, replaced with the text, pasted with Ctrl+V and
/// restored afterwards — unless something else changed it in the meantime.
/// Falls back to typing if the clipboard can't be used.
pub fn insert_text(text: &str, restore_after: Duration) -> Result<(InsertMethod, Instant), String> {
    wait_for_modifiers_released(Duration::from_millis(1500));

    let saved = match save_clipboard() {
        Ok(saved) => saved,
        Err(e) => {
            log::warn!("Dictation: clipboard unavailable ({e}); typing instead");
            return if type_text(text) {
                Ok((InsertMethod::Typed, Instant::now()))
            } else {
                Err("SendInput was blocked".to_string())
            };
        }
    };

    let sequence = match set_clipboard_text(text) {
        Ok(seq) => seq,
        Err(e) => {
            log::warn!("Dictation: could not set clipboard ({e}); typing instead");
            let _ = restore_clipboard(&saved);
            return if type_text(text) {
                Ok((InsertMethod::Typed, Instant::now()))
            } else {
                Err("SendInput was blocked".to_string())
            };
        }
    };

    if !send_ctrl_v() {
        log::warn!("Dictation: Ctrl+V could not be sent; typing instead");
        let _ = restore_clipboard(&saved);
        return if type_text(text) {
            Ok((InsertMethod::Typed, Instant::now()))
        } else {
            Err("SendInput was blocked".to_string())
        };
    }

    let inserted_at = Instant::now();
    std::thread::sleep(restore_after);
    if unsafe { GetClipboardSequenceNumber() } == sequence {
        if let Err(e) = restore_clipboard(&saved) {
            log::warn!("Dictation: could not restore clipboard: {e}");
        }
    } else {
        log::info!("Dictation: clipboard changed after paste; not restoring");
    }
    Ok((InsertMethod::Paste, inserted_at))
}

// ---------------------------------------------------------------------------
// Credential Manager
// ---------------------------------------------------------------------------

pub fn credential_write(target: &str, secret: &str) -> Result<(), String> {
    let mut target_w = wide(target);
    let mut user_w = wide("tingskriver");
    let mut blob = secret.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_w.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user_w.as_mut_ptr()),
        ..Default::default()
    };
    let result = unsafe { CredWriteW(&credential, 0) };
    blob.iter_mut().for_each(|b| *b = 0);
    result.map_err(|e| format!("CredWriteW: {e}"))
}

pub fn credential_read(target: &str) -> Result<Option<String>, String> {
    let target_w = wide(target);
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    let result = unsafe { CredReadW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, None, &mut ptr) };
    if let Err(e) = result {
        if unsafe { GetLastError() } == ERROR_NOT_FOUND || e.code() == ERROR_NOT_FOUND.to_hresult() {
            return Ok(None);
        }
        return Err(format!("CredReadW: {e}"));
    }
    unsafe {
        let credential = &*ptr;
        let secret = if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0 {
            None
        } else {
            let bytes = std::slice::from_raw_parts(credential.CredentialBlob, credential.CredentialBlobSize as usize);
            Some(String::from_utf8_lossy(bytes).into_owned())
        };
        CredFree(ptr as *const c_void);
        Ok(secret)
    }
}

pub fn credential_delete(target: &str) -> Result<(), String> {
    let target_w = wide(target);
    match unsafe { CredDeleteW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(e) => Err(format!("CredDeleteW: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Overlay window
// ---------------------------------------------------------------------------

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

/// Make the overlay a click-through tool window that can never take focus.
pub fn make_overlay_window(raw: isize) {
    unsafe {
        let window = hwnd(raw);
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
        let extra = (WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0 as isize;
        SetWindowLongPtrW(window, GWL_EXSTYLE, style | extra);
    }
}

/// Show the overlay at the bottom centre of the monitor the user is working
/// on, without activating it. Sizes are in logical pixels.
pub fn show_overlay(raw: isize, width: i32, height: i32, margin: i32) {
    unsafe {
        let window = hwnd(raw);
        let dpi = GetDpiForWindow(window).max(96) as i32;
        let scale = |v: i32| v * dpi / 96;
        let (w, h, m) = (scale(width), scale(height), scale(margin));

        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work: RECT = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        };
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.bottom - h - m;
        let _ = SetWindowPos(window, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

pub fn hide_overlay(raw: isize) {
    unsafe {
        let _ = ShowWindow(hwnd(raw), SW_HIDE);
    }
}
