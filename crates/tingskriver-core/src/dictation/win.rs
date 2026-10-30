//! Windows-only pieces of dictation: the global keyboard hook, text insertion
//! via the clipboard (delayed rendering) or `SendInput`, the foreground program, Credential
//! Manager and overlay window styles.
//!
//! Depends only on `std`, `log`, the `windows` crate and `logic`, so it can be
//! type-checked for `x86_64-pc-windows-msvc` without the rest of the app.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, GlobalFree, ERROR_NOT_FOUND, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, RECT,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    DeleteEnhMetaFile, GetEnhMetaFileBits, GetMonitorInfoW, MonitorFromWindow, SetEnhMetaFileBits, HENHMETAFILE,
    MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel, TOKEN_MANDATORY_LABEL,
    TOKEN_QUERY,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
    GetClipboardOwner, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RETURN, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
    GetMessageW, GetWindowLongPtrW, GetWindowThreadProcessId, PeekMessageW, PostThreadMessageW, RegisterClassW,
    SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW, ShowWindow, TranslateMessage, UnhookWindowsHookEx,
    GWL_EXSTYLE, HC_ACTION, HWND_MESSAGE, HWND_TOPMOST, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, PM_REMOVE,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_KEYDOWN, WM_KEYUP,
    WM_QUIT, WM_RENDERALLFORMATS, WM_RENDERFORMAT, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

use super::logic::hotkey::{Chord, ChordSignal, ChordTracker};
use super::logic::paste::{self, FormatKind, Observed, Outcome, SaveCheck, TargetAccess, WhenBlocked};

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

#[derive(Clone, Debug)]
pub struct InsertOptions {
    /// Wait after the target read the text before restoring the clipboard.
    pub restore_after: Duration,
    /// How long the target gets to read the text after Ctrl+V. If it does
    /// not, the field blocks paste and the text is typed.
    pub paste_timeout: Duration,
    /// Program name for messages, e.g. "Code.exe".
    pub app: Option<String>,
}

/// Only one insertion at a time, so one never saves another's text as "the
/// original clipboard".
static INSERT_LOCK: Mutex<()> = Mutex::new(());

const CLIPBOARD_CLASS: &str = "TingskriverClipboardOwner";
/// Time between offering the text and sending Ctrl+V.
const MONITOR_GRACE: Duration = Duration::from_millis(25);

/// The text offered on the clipboard with delayed rendering, and what has
/// happened to it. Lives on the inserting thread, which owns the window.
struct Offer {
    utf16: Vec<u16>,
    paste_sent: bool,
    read_before_paste: bool,
    read_after_paste: Option<Instant>,
}

thread_local! {
    static OFFER: RefCell<Option<Offer>> = const { RefCell::new(None) };
}

/// Hand the offered text to Windows (inside `WM_RENDERFORMAT`, where the
/// clipboard is already open for us). This is the read notification.
fn render_offer() {
    OFFER.with(|offer| {
        let mut offer = offer.borrow_mut();
        let Some(offer) = offer.as_mut() else {
            return;
        };
        let bytes = unsafe { std::slice::from_raw_parts(offer.utf16.as_ptr() as *const u8, offer.utf16.len() * 2) };
        if let Err(e) = unsafe { set_clipboard_bytes(CF_UNICODETEXT.0 as u32, bytes) } {
            log::warn!("Dictation: could not render clipboard text: {e}");
        }
        if offer.paste_sent {
            offer.read_after_paste.get_or_insert_with(Instant::now);
        } else {
            offer.read_before_paste = true;
        }
    });
}

unsafe extern "system" fn clipboard_owner_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_RENDERFORMAT => {
            if wparam.0 as u32 == CF_UNICODETEXT.0 as u32 {
                render_offer();
            }
            LRESULT(0)
        }
        WM_RENDERALLFORMATS => {
            // The window goes away while the text is still only promised.
            if OpenClipboard(Some(hwnd)).is_ok() {
                if GetClipboardOwner().ok() == Some(hwnd) {
                    render_offer();
                }
                let _ = CloseClipboard();
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn register_owner_class() -> Result<(), String> {
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
    REGISTERED
        .get_or_init(|| unsafe {
            let module = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
            let name = wide(CLIPBOARD_CLASS);
            let class = WNDCLASSW {
                lpfnWndProc: Some(clipboard_owner_proc),
                hInstance: module.into(),
                lpszClassName: PCWSTR(name.as_ptr()),
                ..Default::default()
            };
            // The class keeps the name pointer for the life of the process.
            std::mem::forget(name);
            if RegisterClassW(&class) == 0 {
                return Err(format!("RegisterClassW: {}", windows::core::Error::from_win32()));
            }
            Ok(())
        })
        .clone()
}

/// A message-only window that owns the clipboard while the text is on it.
struct ClipboardOwner {
    hwnd: HWND,
}

impl ClipboardOwner {
    fn create() -> Result<ClipboardOwner, String> {
        register_owner_class()?;
        let name = wide(CLIPBOARD_CLASS);
        let hwnd = unsafe {
            let module = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(name.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(module.into()),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW: {e}"))?;
        Ok(ClipboardOwner { hwnd })
    }

    fn open(&self) -> Result<ClipboardGuard, String> {
        // Another program may hold the clipboard for a moment.
        for _ in 0..50 {
            if unsafe { OpenClipboard(Some(self.hwnd)) }.is_ok() {
                return Ok(ClipboardGuard);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err("could not open clipboard".to_string())
    }

    fn owns_clipboard(&self) -> bool {
        unsafe { GetClipboardOwner() }.ok() == Some(self.hwnd)
    }

    /// Handle messages sent to the window (WM_RENDERFORMAT).
    fn pump(&self) {
        let mut msg = MSG::default();
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// Put `text` on the clipboard as a promise (delayed rendering), marked
    /// so clipboard history, cloud clipboard and monitors skip it.
    fn offer(&self, text: &str) -> Result<(), String> {
        OFFER.with(|offer| {
            *offer.borrow_mut() = Some(Offer {
                utf16: text.encode_utf16().chain(std::iter::once(0)).collect(),
                paste_sent: false,
                read_before_paste: false,
                read_after_paste: None,
            })
        });
        let _guard = self.open()?;
        unsafe {
            EmptyClipboard().map_err(|e| format!("EmptyClipboard: {e}"))?;
            // Ownership was taken by EmptyClipboard; a null handle promises
            // the data and Windows asks for it on first read.
            if let Err(e) = SetClipboardData(CF_UNICODETEXT.0 as u32, None) {
                // Some Windows builds return an "error" with a null handle
                // even though the promise is registered; check it is there.
                if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() {
                    return Err(format!("SetClipboardData (delayed): {e}"));
                }
            }
            set_privacy_formats();
        }
        Ok(())
    }

    fn set_paste_sent(&self) {
        OFFER.with(|offer| {
            if let Some(offer) = offer.borrow_mut().as_mut() {
                offer.paste_sent = true;
            }
        });
    }

    fn observed(&self) -> (bool, Option<Instant>) {
        OFFER.with(|offer| {
            offer
                .borrow()
                .as_ref()
                .map(|o| (o.read_before_paste, o.read_after_paste))
                .unwrap_or((false, None))
        })
    }
}

impl Drop for ClipboardOwner {
    fn drop(&mut self) {
        unsafe {
            // Renders a still pending promise (WM_RENDERALLFORMATS) first.
            let _ = DestroyWindow(self.hwnd);
        }
        OFFER.with(|offer| *offer.borrow_mut() = None);
    }
}

enum SavedData {
    Memory(Vec<u8>),
    EnhMetafile(Vec<u8>),
}

struct SavedClipboard {
    formats: Vec<(u32, SavedData)>,
}

impl SavedClipboard {
    fn has_format(&self, format: u32) -> bool {
        self.formats.iter().any(|(f, _)| *f == format)
    }
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn format_name(format: u32) -> Option<String> {
    if format < 0xC000 {
        return None;
    }
    let mut buf = [0u16; 128];
    let len = unsafe { GetClipboardFormatNameW(format, &mut buf) };
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

unsafe fn read_memory_format(format: u32) -> Option<Vec<u8>> {
    let handle = GetClipboardData(format).ok()?;
    let global = HGLOBAL(handle.0);
    let size = GlobalSize(global);
    let ptr = GlobalLock(global) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
    let _ = GlobalUnlock(global);
    Some(bytes)
}

unsafe fn read_enh_metafile() -> Option<Vec<u8>> {
    let handle = GetClipboardData(paste::CF_ENHMETAFILE).ok()?;
    let metafile = HENHMETAFILE(handle.0);
    let size = GetEnhMetaFileBits(metafile, None);
    if size == 0 {
        return None;
    }
    let mut bytes = vec![0u8; size as usize];
    (GetEnhMetaFileBits(metafile, Some(&mut bytes)) == size).then_some(bytes)
}

/// Save every format, or fail so the caller types instead and the original
/// clipboard is never lost.
fn save_clipboard(owner: &ClipboardOwner) -> Result<SavedClipboard, String> {
    let _guard = owner.open()?;
    let mut formats = Vec::new();
    let mut failed = Vec::new();
    let mut total = 0usize;
    unsafe {
        let mut format = EnumClipboardFormats(0);
        while format != 0 {
            let data = match paste::format_kind(format, format_name(format).as_deref()) {
                FormatKind::Skip => None,
                FormatKind::Memory => Some(read_memory_format(format).map(SavedData::Memory)),
                FormatKind::EnhMetafile => Some(read_enh_metafile().map(SavedData::EnhMetafile)),
            };
            match data {
                Some(Some(data)) => {
                    total += match &data {
                        SavedData::Memory(b) | SavedData::EnhMetafile(b) => b.len(),
                    };
                    if total > paste::MAX_SAVED_BYTES {
                        break;
                    }
                    formats.push((format, data));
                }
                Some(None) => failed.push(format),
                None => {}
            }
            format = EnumClipboardFormats(format);
        }
    }
    match paste::check_saved(total, &failed) {
        SaveCheck::Complete => Ok(SavedClipboard { formats }),
        SaveCheck::TypeInstead(reason) => Err(reason),
    }
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

unsafe fn set_enh_metafile(bytes: &[u8]) -> Result<(), String> {
    let metafile = SetEnhMetaFileBits(bytes);
    if metafile.is_invalid() {
        return Err("SetEnhMetaFileBits failed".to_string());
    }
    if let Err(e) = SetClipboardData(paste::CF_ENHMETAFILE, Some(HANDLE(metafile.0))) {
        let _ = DeleteEnhMetaFile(Some(metafile));
        return Err(format!("SetClipboardData (metafile): {e}"));
    }
    Ok(())
}

fn registered_format(name: &str) -> u32 {
    let name = wide(name);
    unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
}

/// Keep the current clipboard entry out of Win+V history, cloud clipboard and
/// clipboard monitors. The clipboard must be open and owned.
unsafe fn set_privacy_formats() {
    let zero = 0u32.to_le_bytes();
    for name in paste::PRIVACY_FORMATS {
        let format = registered_format(name);
        if format != 0 {
            let _ = set_clipboard_bytes(format, &zero);
        }
    }
}

/// Put the saved formats back. They are marked like the dictated text, so
/// clipboard history and cloud clipboard (which already have the original)
/// do not get a duplicate.
fn restore_clipboard(owner: &ClipboardOwner, saved: &SavedClipboard) -> Result<(), String> {
    let _guard = owner.open()?;
    unsafe {
        EmptyClipboard().map_err(|e| format!("EmptyClipboard: {e}"))?;
        for (format, data) in &saved.formats {
            let result = match data {
                SavedData::Memory(bytes) => set_clipboard_bytes(*format, bytes),
                SavedData::EnhMetafile(bytes) => set_enh_metafile(bytes),
            };
            if let Err(e) = result {
                log::warn!("Dictation: could not restore clipboard format {format}: {e}");
            }
        }
        if !saved.formats.is_empty() {
            for name in paste::PRIVACY_FORMATS {
                let format = registered_format(name);
                if format != 0 && !saved.has_format(format) {
                    let _ = set_clipboard_bytes(format, &0u32.to_le_bytes());
                }
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

fn typed(text: &str) -> Result<(InsertMethod, Instant), String> {
    if type_text(text) {
        Ok((InsertMethod::Typed, Instant::now()))
    } else {
        Err("Windows afviste tastetryk fra Tingskriver (SendInput)".to_string())
    }
}

/// Mandatory integrity level of a process (0x2000 medium, 0x3000 high).
fn integrity_level(process: HANDLE) -> Option<u32> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut len);
        // u64 storage keeps TOKEN_MANDATORY_LABEL aligned.
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let read = GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some(buf.as_mut_ptr() as *mut c_void),
            (buf.len() * 8) as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        read.ok()?;
        let label = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        Some(*GetSidSubAuthority(sid, (count - 1) as u32))
    }
}

fn foreground_integrity_level() -> Option<u32> {
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
        let level = integrity_level(process);
        let _ = CloseHandle(process);
        level
    }
}

/// Insert `text` in the focused program. Blocks until the original clipboard
/// is back. Returns how the text was inserted and when the program got it.
///
/// The original clipboard (all formats, also images) is saved, the text is
/// offered with delayed rendering and pasted with Ctrl+V. When the target
/// reads it (`WM_RENDERFORMAT`), the original is restored `restore_after`
/// later, unless another program changed the clipboard in between. If the
/// target does not read it within `paste_timeout`, the field blocks paste:
/// the original is restored and the text is typed with `SendInput`.
pub fn insert_text(text: &str, options: &InsertOptions) -> Result<(InsertMethod, Instant), String> {
    let _one_at_a_time = INSERT_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    wait_for_modifiers_released(Duration::from_millis(1500));

    let own = integrity_level(unsafe { GetCurrentProcess() });
    let access = paste::target_access(own, foreground_integrity_level());
    if access == TargetAccess::Elevated {
        log::warn!("Dictation: target program runs as administrator; not inserting");
        return Err(paste::elevated_message(options.app.as_deref()));
    }

    let owner = match ClipboardOwner::create() {
        Ok(owner) => owner,
        Err(e) => {
            log::warn!("Dictation: no clipboard window ({e}); typing instead");
            return typed(text);
        }
    };
    let saved = match save_clipboard(&owner) {
        Ok(saved) => saved,
        Err(e) => {
            log::warn!("Dictation: clipboard not saved ({e}); typing instead");
            return typed(text);
        }
    };
    if let Err(e) = owner.offer(text) {
        log::warn!("Dictation: could not set clipboard ({e}); typing instead");
        let _ = restore_clipboard(&owner, &saved);
        return typed(text);
    }

    // A clipboard monitor that ignores the exclusion format reads right after
    // the clipboard changed; let it do so before Ctrl+V, so its read is not
    // taken for the target's.
    let offered_at = Instant::now();
    while offered_at.elapsed() < MONITOR_GRACE {
        owner.pump();
        std::thread::sleep(Duration::from_millis(2));
    }
    owner.set_paste_sent();
    let sent_at = Instant::now();
    let ctrl_v_sent = send_ctrl_v();
    if ctrl_v_sent {
        while sent_at.elapsed() < options.paste_timeout {
            owner.pump();
            if owner.observed().1.is_some() || !owner.owns_clipboard() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let (read_before_paste, read_after_paste) = owner.observed();
    let outcome = paste::outcome(Observed {
        read_before_paste,
        ctrl_v_sent,
        read_after_paste: read_after_paste.is_some(),
        replaced: !owner.owns_clipboard(),
    });
    log::info!(
        "Dictation paste: {outcome:?} after {} ms (target read {})",
        sent_at.elapsed().as_millis(),
        read_after_paste
            .map(|at| format!("{} ms after Ctrl+V", at.saturating_duration_since(sent_at).as_millis()))
            .unwrap_or_else(|| "nothing".to_string())
    );

    if outcome.restore() {
        let wait = match outcome {
            Outcome::Pasted => options.restore_after,
            Outcome::PastedUnconfirmed => options.paste_timeout.max(options.restore_after),
            _ => Duration::ZERO,
        };
        let until = Instant::now() + wait;
        while Instant::now() < until {
            owner.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        if owner.owns_clipboard() {
            if let Err(e) = restore_clipboard(&owner, &saved) {
                log::warn!("Dictation: could not restore clipboard: {e}");
            }
        } else {
            log::info!("Dictation: clipboard changed after paste; not restoring");
        }
    }

    match outcome {
        Outcome::Pasted => Ok((InsertMethod::Paste, read_after_paste.unwrap_or(sent_at))),
        Outcome::PastedUnconfirmed | Outcome::Replaced { typed: false } => Ok((InsertMethod::Paste, sent_at)),
        Outcome::Blocked | Outcome::Replaced { typed: true } => match paste::when_blocked(access) {
            WhenBlocked::Type => {
                log::info!("Dictation: the field did not take Ctrl+V; typing instead");
                typed(text)
            }
            WhenBlocked::Report => Err(paste::not_received_message(options.app.as_deref())),
        },
    }
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

// ---------------------------------------------------------------------------
// Insertion test against a real window (Windows desktop only)
// ---------------------------------------------------------------------------

/// Runs only on request, on a Windows desktop session (it types into a test
/// window and uses the real clipboard):
///
/// ```text
/// cargo test -p tingskriver-core --lib insertion_window -- --ignored --test-threads=1
/// ```
#[cfg(test)]
mod insertion_window {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GetWindowTextLengthW, GetWindowTextW, SendMessageW, SetForegroundWindow,
        SetWindowTextW, ShowWindow as Show, ES_AUTOHSCROLL, GWLP_WNDPROC, SW_SHOW, WM_PASTE, WNDPROC,
        WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;

    /// Raw handles cross threads as integers.
    #[derive(Clone, Copy)]
    struct Handles {
        edit: isize,
    }

    static ORIGINAL_EDIT_PROC: OnceLock<isize> = OnceLock::new();

    /// An edit field that ignores paste, like a password or "retype" field.
    unsafe extern "system" fn paste_blocking_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_PASTE {
            return LRESULT(0);
        }
        let original: WNDPROC = ORIGINAL_EDIT_PROC.get().and_then(|p| std::mem::transmute::<isize, WNDPROC>(*p));
        CallWindowProcW(original, hwnd, msg, wparam, lparam)
    }

    /// Start a window with an edit field on its own thread with a message loop.
    fn test_window(block_paste: bool) -> Handles {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || unsafe {
            let edit_class = wide("EDIT");
            let title = wide("Tingskriver insertion test");
            let module = GetModuleHandleW(None).expect("module");
            // A top-level edit control: the window and the field are one.
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(edit_class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                200,
                200,
                700,
                120,
                None,
                None,
                Some(module.into()),
                None,
            )
            .expect("edit window");
            if block_paste {
                let original = SetWindowLongPtrW(edit, GWLP_WNDPROC, paste_blocking_proc as *const () as isize);
                let _ = ORIGINAL_EDIT_PROC.set(original);
            }
            let _ = Show(edit, SW_SHOW);
            // An Alt tap lets a background process take the foreground.
            send_inputs(&[
                key_input(VK_MENU.0, 0, KEYBD_EVENT_FLAGS(0)),
                key_input(VK_MENU.0, 0, KEYEVENTF_KEYUP),
            ]);
            let _ = SetForegroundWindow(edit);
            let _ = SetFocus(Some(edit));
            tx.send(Handles { edit: edit.0 as isize }).expect("report window");
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let handles = rx.recv_timeout(Duration::from_secs(5)).expect("test window started");
        std::thread::sleep(Duration::from_millis(300));
        handles
    }

    fn edit_text(handles: Handles) -> String {
        unsafe {
            let edit = hwnd(handles.edit);
            let len = GetWindowTextLengthW(edit).max(0) as usize;
            let mut buf = vec![0u16; len + 1];
            let read = GetWindowTextW(edit, &mut buf).max(0) as usize;
            String::from_utf16_lossy(&buf[..read])
        }
    }

    fn clear(handles: Handles) {
        let empty = wide("");
        unsafe {
            let _ = SetWindowTextW(hwnd(handles.edit), PCWSTR(empty.as_ptr()));
            let _ = SendMessageW(hwnd(handles.edit), 0x00B1 /* EM_SETSEL */, Some(WPARAM(0)), Some(LPARAM(0)));
        }
    }

    /// A 3000 x 2000 32-bit DIB (24 MB) and some text, like a screenshot
    /// copied from an image editor.
    fn put_original(owner: &ClipboardOwner) -> Vec<u8> {
        let (w, h) = (3000i32, 2000i32);
        let mut dib = Vec::with_capacity(40 + (w * h * 4) as usize);
        for v in [40i32, w, h] {
            dib.extend_from_slice(&v.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes()); // planes
        dib.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        dib.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        dib.extend_from_slice(&((w * h * 4) as u32).to_le_bytes());
        dib.extend_from_slice(&[0u8; 16]);
        dib.extend((0..(w * h * 4) as usize).map(|i| (i % 251) as u8));
        let text: Vec<u16> = "ORIGINAL æøå".encode_utf16().chain(std::iter::once(0)).collect();
        let _guard = owner.open().expect("open clipboard");
        unsafe {
            EmptyClipboard().expect("empty clipboard");
            set_clipboard_bytes(8 /* CF_DIB */, &dib).expect("set DIB");
            let bytes = std::slice::from_raw_parts(text.as_ptr() as *const u8, text.len() * 2);
            set_clipboard_bytes(CF_UNICODETEXT.0 as u32, bytes).expect("set text");
        }
        dib
    }

    fn read_back(owner: &ClipboardOwner) -> (String, Vec<u8>) {
        let _guard = owner.open().expect("open clipboard");
        unsafe {
            let text = read_memory_format(CF_UNICODETEXT.0 as u32).unwrap_or_default();
            let units: Vec<u16> = text.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let text = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
            (text, read_memory_format(8).unwrap_or_default())
        }
    }

    fn options() -> InsertOptions {
        InsertOptions {
            restore_after: Duration::from_millis(300),
            paste_timeout: Duration::from_millis(1000),
            app: Some("test".into()),
        }
    }

    #[test]
    #[ignore = "needs a Windows desktop; types into a test window"]
    fn fifty_dictations_in_a_row_keep_text_and_clipboard_right() {
        let handles = test_window(false);
        let setup = ClipboardOwner::create().expect("clipboard window");
        let dib = put_original(&setup);
        for i in 0..50 {
            clear(handles);
            let text = format!("Diktat nummer {i}: æøå, ÆØÅ og «citat».");
            let (method, _) = insert_text(&text, &options()).expect("insert");
            std::thread::sleep(Duration::from_millis(50));
            assert_eq!(edit_text(handles), text, "dictation {i} gave wrong or stale text");
            assert_eq!(method, InsertMethod::Paste, "dictation {i} was not pasted");
            let (clip_text, clip_dib) = read_back(&setup);
            assert_eq!(clip_text, "ORIGINAL æøå", "clipboard text changed after dictation {i}");
            assert!(
                clip_dib.len() >= dib.len() && clip_dib[..dib.len()] == dib[..],
                "clipboard image changed after dictation {i}"
            );
        }
    }

    #[test]
    #[ignore = "needs a Windows desktop; types into a test window"]
    fn field_blocking_paste_gets_text_typed() {
        let handles = test_window(true);
        let setup = ClipboardOwner::create().expect("clipboard window");
        put_original(&setup);
        clear(handles);
        let text = "Skrevet med SendInput: æøå";
        let (method, _) = insert_text(text, &options()).expect("insert");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(method, InsertMethod::Typed);
        assert_eq!(edit_text(handles), text);
        assert_eq!(read_back(&setup).0, "ORIGINAL æøå");
    }
}
