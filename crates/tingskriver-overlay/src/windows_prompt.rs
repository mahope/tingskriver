//! The prompt window as a native Win32 popup (Windows).
//!
//! Like the pill it lives on its own thread with its own message loop, and
//! callers on any thread talk to it over a channel ([`PromptWindow`]).
//!
//! **No focus stealing.** The popup is `WS_EX_NOACTIVATE | WS_EX_TOPMOST |
//! WS_EX_TOOLWINDOW`, shown with `SW_SHOWNOACTIVATE`, and answers
//! `WM_MOUSEACTIVATE` with `MA_NOACTIVATE`, so neither showing it nor
//! clicking a button moves the foreground (the dictation target keeps the
//! caret). Keyboard access that does not need focus is a set of global
//! shortcuts, registered only while a prompt is on screen
//! ([`Shortcut`]). `Ctrl+Alt+P` is the one deliberate exception: the user
//! asks for focus, the popup takes it, Tab / Enter / Space / Esc work, and
//! the previous foreground window gets it back when the prompt closes.
//!
//! **Screen readers.** The title and body are real `STATIC` controls and the
//! actions real `BUTTON` controls with their labels as text, and the popup
//! has a window name, so UI Automation and MSAA expose names, roles and
//! the focus without a hand-written provider.
//!
//! **Focus Assist.** Before a prompt is shown, [`query_notification_state`]
//! feeds [`crate::focus::decide`]; a blocked prompt waits (and the pill gets
//! a mark in quiet time) until it is out of date.
//!
//! Layout and decisions are pure and tested elsewhere
//! ([`crate::prompt_layout`], [`crate::prompt`], [`crate::focus`]); this file
//! is the thin Win32 shell around them.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Once;
use std::thread::JoinHandle;
use std::time::Instant;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    FrameRect, GetDC, InvalidateRect, ReleaseDC, ScreenToClient, SelectObject, SetBkColor,
    SetTextColor, DT_CALCRECT, DT_NOPREFIX, DT_WORDBREAK, HBRUSH, HDC, HFONT, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::UI::Controls::SetWindowTheme;
use windows_sys::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, RegisterHotKey, SetFocus, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::Shell::SHQueryUserNotificationState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetWindowRect, IsChild, IsWindow, KillTimer, PostMessageW,
    PostQuitMessage, RegisterClassExW, SendMessageW, SetForegroundWindow, SetTimer, SetWindowPos,
    ShowWindow, TranslateMessage, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, HWND_MESSAGE, HWND_TOPMOST,
    MA_NOACTIVATE, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WA_INACTIVE,
    WM_ACTIVATE, WM_APP, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY, WM_ERASEBKGND,
    WM_HOTKEY, WM_KEYDOWN, WM_MOUSEACTIVATE, WM_PAINT, WM_SETFONT, WM_TIMER, WNDCLASSEXW, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_TABSTOP,
    WS_VISIBLE,
};

use crate::focus::{self, Decision, NotificationState};
use crate::layout::ScreenRect;
use crate::prompt::{Key, PromptId, PromptModel, PromptOutcome, PromptRequest, Shortcut};
use crate::prompt_layout::{place, size, Hit, PromptLayout};
use crate::windows::{current_monitor, Monitor};

const WM_WAKE: u32 = WM_APP + 1;
const TIMER: usize = 1;
/// How often countdown, hover and held prompts are looked at.
const TICK_MS: u32 = 100;
const BUTTON_ID: usize = 100;
/// `STATIC` styles (left aligned, no `&` accelerators in the text).
const SS_LEFT: u32 = 0;
const SS_NOPREFIX: u32 = 0x80;
const HOTKEY_BASE: i32 = 0x7A00;

/// What the prompt window reports to its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptEvent {
    /// The prompt was answered, dismissed or ran out.
    Answered {
        id: PromptId,
        outcome: PromptOutcome,
    },
    /// The prompt was held back until it was out of date and never shown.
    Dropped { id: PromptId },
    /// A prompt is (no longer) held back by Focus Assist: the pill should
    /// (stop) show(ing) its mark.
    Held(bool),
}

enum Command {
    Show(PromptRequest, Instant),
    Cancel(PromptId),
    Quit,
}

/// The prompt window. Cheap to call from any thread. Prompts are shown one
/// at a time, in the order they were requested. Dropping it closes it.
pub struct PromptWindow {
    tx: Sender<Command>,
    host: isize,
    events: Receiver<PromptEvent>,
    thread: Option<JoinHandle<()>>,
}

impl PromptWindow {
    pub fn new() -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("tingskriver-prompt".into())
            .spawn(move || ui_thread(rx, events_tx, ready_tx))
            .map_err(|e| format!("could not start the prompt thread: {e}"))?;
        let host = ready_rx
            .recv()
            .map_err(|_| "the prompt thread stopped before it was ready".to_string())??;
        Ok(Self {
            tx,
            host,
            events,
            thread: Some(thread),
        })
    }

    fn send(&self, command: Command) {
        if self.tx.send(command).is_ok() {
            // SAFETY: posting to a window handle is safe even if it is gone.
            unsafe { PostMessageW(self.host as HWND, WM_WAKE, 0, 0) };
        }
    }

    /// Ask the user. It is shown as soon as Windows' notification state
    /// allows it and no other prompt is up, or dropped when `request.ttl`
    /// has passed first (see [`PromptEvent::Dropped`]).
    pub fn show(&self, request: PromptRequest) {
        self.send(Command::Show(request, Instant::now()));
    }

    /// Withdraw a prompt (waiting or on screen) without an answer, for
    /// example when the meeting ended by itself.
    pub fn cancel(&self, id: PromptId) {
        self.send(Command::Cancel(id));
    }

    /// Answers and status changes, oldest first.
    pub fn events(&self) -> &Receiver<PromptEvent> {
        &self.events
    }

    /// The hidden host window, for tests.
    pub fn host_hwnd(&self) -> isize {
        self.host
    }
}

impl Drop for PromptWindow {
    fn drop(&mut self) {
        self.send(Command::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Windows' state for "may I disturb the user" (`None` when the query
/// fails, which [`focus::decide`] treats as "show").
pub fn query_notification_state() -> Option<NotificationState> {
    let mut raw: i32 = 0;
    // SAFETY: one out-pointer to an i32-sized enum.
    let hr = unsafe { SHQueryUserNotificationState(&mut raw) };
    (hr >= 0).then(|| NotificationState::from_raw(raw))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

/// Colours as `COLORREF`.
#[derive(Clone, Copy)]
struct Palette {
    background: u32,
    text: u32,
    border: u32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            // Same tokens as the pill (`--card`, `--card-foreground`, `--border`).
            Self {
                background: rgb(0x1a, 0x1d, 0x23),
                text: rgb(0xec, 0xea, 0xe6),
                border: rgb(0x31, 0x35, 0x3f),
            }
        } else {
            Self {
                background: rgb(0xf7, 0xf7, 0xf8),
                text: rgb(0x1b, 0x1d, 0x22),
                border: rgb(0xd0, 0xd3, 0xd9),
            }
        }
    }
}

/// Windows' "app mode" (`AppsUseLightTheme` = 0 means dark).
fn apps_use_dark_mode() -> bool {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let value = wide("AppsUseLightTheme");
    let mut data: u32 = 1;
    let mut len = std::mem::size_of::<u32>() as u32;
    // SAFETY: valid NUL-terminated names and one DWORD of output.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut data as *mut u32 as *mut core::ffi::c_void,
            &mut len,
        )
    };
    status == 0 && data == 0
}

type UiAction = Box<dyn FnOnce(&mut Ui)>;

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static PENDING: RefCell<VecDeque<UiAction>> = const { RefCell::new(VecDeque::new()) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let result = UI.with(|ui| {
        let mut ui = ui.try_borrow_mut().ok()?;
        ui.as_mut().map(f)
    });
    // Calls made while processing an action can enqueue more work. Pop without
    // holding the queue's borrow, and keep the UI borrowed until it is empty.
    UI.with(|ui| {
        if let Ok(mut ui) = ui.try_borrow_mut() {
            if let Some(ui) = ui.as_mut() {
                loop {
                    let action = PENDING.with(|pending| pending.borrow_mut().pop_front());
                    let Some(action) = action else { break };
                    action(ui);
                }
            }
        }
    });
    result
}

fn ui_action(action: impl FnOnce(&mut Ui) + 'static) {
    PENDING.with(|pending| pending.borrow_mut().push_back(Box::new(action)));
    with_ui(|_| ());
}

fn repaint_later(hwnd: HWND) {
    ui_action(move |ui| {
        if ui.active.as_ref().is_some_and(|active| active.hwnd == hwnd) {
            // HDCs from synchronous paint messages cannot outlive the callback.
            unsafe { InvalidateRect(hwnd, null(), 1) };
        }
    });
}

fn ui_thread(
    rx: Receiver<Command>,
    events: Sender<PromptEvent>,
    ready: Sender<Result<isize, String>>,
) {
    // SAFETY: plain Win32 calls on this thread.
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let host = match create_host() {
        Ok(host) => host,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    UI.with(|ui| *ui.borrow_mut() = Some(Ui::new(host, rx, events)));
    let _ = ready.send(Ok(host as isize));

    // SAFETY: standard message loop on the thread that owns the windows.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if with_ui(|ui| ui.intercept_key(&msg)).unwrap_or(false) {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // Dropping the Ui closes any prompt and unregisters the shortcuts.
    UI.with(|ui| ui.borrow_mut().take());
}

fn register_classes() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        // SAFETY: class data outlives the call; the procedures have the right
        // signature.
        unsafe {
            let instance = GetModuleHandleW(null());
            for (name, proc) in [
                ("TingskriverPromptVaert", host_proc as _),
                ("TingskriverPrompt", prompt_proc as _),
            ] {
                let class = wide(name);
                let mut wc: WNDCLASSEXW = std::mem::zeroed();
                wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
                wc.lpfnWndProc = Some(proc);
                wc.hInstance = instance;
                wc.lpszClassName = class.as_ptr();
                RegisterClassExW(&wc);
            }
        }
    });
}

/// The hidden message-only window that receives wake-ups, the timer and the
/// global shortcuts.
fn create_host() -> Result<HWND, String> {
    register_classes();
    let class = wide("TingskriverPromptVaert");
    // SAFETY: valid class name; a message-only window has no visuals.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    if hwnd.is_null() {
        return Err(format!(
            "could not create the prompt host window: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(hwnd)
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_WAKE => {
            ui_action(Ui::drain);
            0
        }
        WM_TIMER if wparam == TIMER => {
            ui_action(Ui::on_timer);
            0
        }
        WM_HOTKEY => {
            let index = wparam as i32 - HOTKEY_BASE;
            if let Some(shortcut) = usize::try_from(index)
                .ok()
                .and_then(|i| Shortcut::ALL.get(i).copied())
            {
                ui_action(move |ui| ui.on_shortcut(shortcut));
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn prompt_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // Never activate on a click; the dictation target keeps the focus.
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_COMMAND => {
            let id = wparam & 0xffff;
            if id >= BUTTON_ID {
                ui_action(move |ui| {
                    if ui.active.as_ref().is_some_and(|active| active.hwnd == hwnd) {
                        ui.on_button(id - BUTTON_ID);
                    }
                });
            }
            0
        }
        WM_ACTIVATE => {
            let active = (wparam & 0xffff) as u32 != WA_INACTIVE;
            ui_action(move |ui| {
                if ui.active.as_ref().is_some_and(|a| a.hwnd == hwnd) {
                    ui.on_keyboard_focus(active);
                }
            });
            0
        }
        WM_ERASEBKGND => {
            let mut area: RECT = std::mem::zeroed();
            windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut area);
            if let Some(brush) = with_ui(|ui| ui.background_brush()).flatten() {
                FillRect(wparam as HDC, &area, brush);
                1
            } else {
                repaint_later(hwnd);
                0
            }
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut area: RECT = std::mem::zeroed();
            windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut area);
            if let Some(brush) = with_ui(|ui| ui.border_brush()).flatten() {
                FrameRect(hdc, &area, brush);
            } else {
                repaint_later(hwnd);
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => match with_ui(|ui| ui.control_colors()).flatten() {
            Some((text, background, brush)) => {
                SetTextColor(wparam as HDC, text);
                SetBkColor(wparam as HDC, background);
                brush as LRESULT
            }
            None => {
                repaint_later(hwnd);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        },
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// A prompt that is waiting for the notification state to allow it.
struct Waiting {
    request: PromptRequest,
    since: Instant,
    next_check: Instant,
}

/// The prompt on screen and the Win32 objects it owns.
struct Active {
    hwnd: HWND,
    /// The host window that owns the global shortcuts (WM_HOTKEY goes to it).
    host: HWND,
    model: PromptModel,
    layout: PromptLayout,
    monitor: Monitor,
    body: HWND,
    shown_body: Option<String>,
    buttons: Vec<HWND>,
    fonts: [HFONT; 2],
    background: HBRUSH,
    border: HBRUSH,
    palette: Palette,
    /// Foreground window before the user moved focus into the prompt.
    previous_foreground: HWND,
    has_keyboard_focus: bool,
}

impl Drop for Active {
    fn drop(&mut self) {
        // SAFETY: our own window and GDI objects, on the thread that made them.
        unsafe {
            for shortcut_index in 0..Shortcut::ALL.len() {
                UnregisterHotKey(self.host, HOTKEY_BASE + shortcut_index as i32);
            }
            DestroyWindow(self.hwnd);
            for font in self.fonts {
                DeleteObject(font as _);
            }
            DeleteObject(self.background as _);
            DeleteObject(self.border as _);
        }
    }
}

struct Ui {
    host: HWND,
    rx: Receiver<Command>,
    events: Sender<PromptEvent>,
    waiting: VecDeque<Waiting>,
    active: Option<Active>,
    held: bool,
    timer_on: bool,
}

impl Ui {
    fn new(host: HWND, rx: Receiver<Command>, events: Sender<PromptEvent>) -> Self {
        Self {
            host,
            rx,
            events,
            waiting: VecDeque::new(),
            active: None,
            held: false,
            timer_on: false,
        }
    }

    fn emit(&self, event: PromptEvent) {
        // The owner may have dropped the receiver; nothing to tell then.
        let _ = self.events.send(event);
    }

    fn drain(&mut self) {
        while let Ok(command) = self.rx.try_recv() {
            match command {
                Command::Show(request, at) => self.waiting.push_back(Waiting {
                    request,
                    since: at,
                    next_check: at,
                }),
                Command::Cancel(id) => {
                    self.waiting.retain(|w| w.request.id != id);
                    if self
                        .active
                        .as_ref()
                        .is_some_and(|a| a.model.request().id == id)
                    {
                        self.release_active();
                    }
                }
                Command::Quit => {
                    self.active = None;
                    self.stop_timer();
                    // SAFETY: our own window, on its thread.
                    unsafe { DestroyWindow(self.host) };
                    return;
                }
            }
        }
        self.pump(Instant::now());
    }

    fn on_timer(&mut self) {
        let now = Instant::now();
        if let Some(active) = &mut self.active {
            // SAFETY: plain queries with valid out-pointers.
            let hovered = unsafe {
                let mut cursor = POINT { x: 0, y: 0 };
                GetCursorPos(&mut cursor);
                ScreenToClient(active.hwnd, &mut cursor);
                active.layout.hit_test(cursor.x, cursor.y) != Hit::Outside
            };
            active.model.set_hover(hovered, now);
            if let Some(outcome) = active.model.tick(now) {
                self.finish(outcome);
            } else {
                self.refresh_body(now);
            }
        }
        self.pump(now);
    }

    /// Show the next waiting prompt when nothing is on screen, as far as
    /// Windows' notification state allows, and keep the timer running only
    /// while there is something to watch.
    fn pump(&mut self, now: Instant) {
        while self.active.is_none() {
            let Some(front) = self.waiting.front() else {
                break;
            };
            if now < front.next_check {
                break;
            }
            let waited = now.saturating_duration_since(front.since);
            let decision = focus::decide(query_notification_state(), waited, front.request.ttl);
            match decision {
                Decision::Show => {
                    let Some(waiting) = self.waiting.pop_front() else {
                        break;
                    };
                    self.set_held(false);
                    let id = waiting.request.id;
                    match self.open(waiting.request, now) {
                        Ok(active) => self.active = Some(active),
                        Err(e) => {
                            // Tell the owner, or it waits for an answer forever.
                            log::warn!("Prompt {id}: {e}");
                            self.emit(PromptEvent::Dropped { id });
                        }
                    }
                }
                Decision::Defer {
                    retry_in,
                    mark_pill,
                } => {
                    if let Some(front) = self.waiting.front_mut() {
                        front.next_check = now + retry_in;
                    }
                    self.set_held(mark_pill);
                    break;
                }
                Decision::Drop { mark_pill } => {
                    if let Some(dropped) = self.waiting.pop_front() {
                        log::info!("Prompt {} dropped, out of date", dropped.request.id);
                        self.emit(PromptEvent::Dropped {
                            id: dropped.request.id,
                        });
                    }
                    // The mark stays only while another prompt is held back.
                    self.set_held(mark_pill && !self.waiting.is_empty());
                }
            }
        }
        if self.waiting.is_empty() && self.active.is_none() {
            self.set_held(false);
            self.stop_timer();
        } else {
            self.start_timer();
        }
    }

    fn set_held(&mut self, held: bool) {
        if self.held != held {
            self.held = held;
            self.emit(PromptEvent::Held(held));
        }
    }

    fn start_timer(&mut self) {
        if !self.timer_on {
            // SAFETY: timer on our own window.
            unsafe { SetTimer(self.host, TIMER, TICK_MS, None) };
            self.timer_on = true;
        }
    }

    fn stop_timer(&mut self) {
        if self.timer_on {
            // SAFETY: timer on our own window.
            unsafe { KillTimer(self.host, TIMER) };
            self.timer_on = false;
        }
    }

    /// Build and show the popup for `request`, without activating it.
    fn open(&mut self, request: PromptRequest, now: Instant) -> Result<Active, String> {
        let monitor = current_monitor();
        let scale = monitor.scale;
        let palette = Palette::new(apps_use_dark_mode());
        let model = PromptModel::new(request, now);

        // SAFETY: GDI objects and windows are created and used on this
        // thread; every handle is checked or owned by `Active`.
        unsafe {
            let title_font = make_font(14.0 * scale, 600);
            let body_font = make_font(13.0 * scale, 400);
            let background = CreateSolidBrush(palette.background);
            let border = CreateSolidBrush(palette.border);

            // Count the body's lines at the real width and font.
            let body_text = model.body_text(now);
            let text_width = crate::prompt_layout::text_width(scale);
            let lines = match &body_text {
                Some(text) => count_lines(body_font, text, text_width, scale),
                None => 0,
            };
            let work_h = monitor.work.bottom - monitor.work.top;
            let layout = PromptLayout::new(scale, lines, model.request().buttons.len(), work_h);

            let (x, y) = place(monitor.work, layout.width, layout.height, scale);
            let class = wide("TingskriverPrompt");
            let name = wide(&model.window_name());
            let instance = GetModuleHandleW(null());
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                name.as_ptr(),
                WS_POPUP | WS_CLIPCHILDREN,
                x,
                y,
                layout.width,
                layout.height,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                for object in [
                    title_font as _,
                    body_font as _,
                    background as _,
                    border as _,
                ] {
                    DeleteObject(object);
                }
                return Err(format!(
                    "could not create the prompt window: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let child = |class: &str,
                         text: &str,
                         style: u32,
                         r: crate::prompt_layout::PxRect,
                         id: usize,
                         font: HFONT| {
                let class = wide(class);
                let text = wide(text);
                let h = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    text.as_ptr(),
                    WS_CHILD | WS_VISIBLE | style,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    hwnd,
                    id as _,
                    instance,
                    null(),
                );
                SendMessageW(h, WM_SETFONT, font as WPARAM, 1);
                h
            };

            let static_style = SS_LEFT | SS_NOPREFIX;
            child(
                "STATIC",
                &model.request().title,
                static_style,
                layout.title,
                0,
                title_font,
            );
            let body = match (layout.body, &body_text) {
                (Some(rect), Some(text)) => child("STATIC", text, static_style, rect, 0, body_font),
                _ => null_mut(),
            };
            let mut buttons = Vec::new();
            for (i, rect) in layout.buttons.iter().enumerate() {
                let label = model.button_name(i).unwrap_or_default();
                let kind = if i == 0 {
                    BS_DEFPUSHBUTTON
                } else {
                    BS_PUSHBUTTON
                } as u32;
                let h = child(
                    "BUTTON",
                    label,
                    kind | WS_TABSTOP,
                    *rect,
                    BUTTON_ID + i,
                    body_font,
                );
                let theme = wide(if palette.background == Palette::new(true).background {
                    "DarkMode_Explorer"
                } else {
                    "Explorer"
                });
                SetWindowTheme(h, theme.as_ptr(), null());
                buttons.push(h);
            }

            // Shortcuts that need no focus; a failure (taken by another app)
            // only costs that shortcut.
            for (i, shortcut) in Shortcut::ALL.iter().enumerate() {
                // Registered on the host window, whose procedure handles
                // WM_HOTKEY; the prompt window's procedure does not.
                let ok = RegisterHotKey(
                    self.host,
                    HOTKEY_BASE + i as i32,
                    MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                    shortcut.virtual_key(),
                );
                if ok == 0 {
                    log::warn!("Prompt: could not register shortcut {shortcut:?}");
                }
            }

            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );

            Ok(Active {
                hwnd,
                host: self.host,
                model,
                layout,
                monitor,
                body,
                shown_body: body_text,
                buttons,
                fonts: [title_font, body_font],
                background,
                border,
                palette,
                previous_foreground: null_mut(),
                has_keyboard_focus: false,
            })
        }
    }

    /// Keep a visible countdown's text current (only when it changed).
    fn refresh_body(&mut self, now: Instant) {
        let Some(active) = &mut self.active else {
            return;
        };
        if active.body.is_null() {
            return;
        }
        let text = active.model.body_text(now);
        if text != active.shown_body {
            if let Some(text) = &text {
                let w = wide(text);
                // SAFETY: our own child window.
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW(
                        active.body,
                        w.as_ptr(),
                    )
                };
            }
            active.shown_body = text;
        }
    }

    /// Close the active prompt and, if it had keyboard focus, hand the
    /// foreground back to the window the user came from. Returns the id.
    fn release_active(&mut self) -> Option<PromptId> {
        let active = self.active.take()?;
        let id = active.model.request().id;
        let give_back = active
            .has_keyboard_focus
            .then_some(active.previous_foreground);
        drop(active);
        if let Some(previous) = give_back {
            // SAFETY: the handle is checked with IsWindow first.
            unsafe {
                if !previous.is_null() && IsWindow(previous) != 0 {
                    SetForegroundWindow(previous);
                }
            }
        }
        Some(id)
    }

    fn finish(&mut self, outcome: PromptOutcome) {
        let Some(id) = self.release_active() else {
            return;
        };
        self.emit(PromptEvent::Answered { id, outcome });
        self.pump(Instant::now());
    }

    fn on_button(&mut self, index: usize) {
        let outcome = self.active.as_mut().and_then(|a| a.model.click(index));
        if let Some(outcome) = outcome {
            self.finish(outcome);
        }
    }

    fn on_keyboard_focus(&mut self, focused: bool) {
        if let Some(active) = &mut self.active {
            active.has_keyboard_focus = focused;
            active.model.set_keyboard_focus(focused, Instant::now());
        }
    }

    fn on_shortcut(&mut self, shortcut: Shortcut) {
        let Some(active) = &mut self.active else {
            return;
        };
        if shortcut == Shortcut::FocusWindow {
            // The user asked for this, so taking focus is allowed.
            // SAFETY: our own windows; handles are checked.
            unsafe {
                let foreground = GetForegroundWindow();
                if foreground != active.hwnd {
                    active.previous_foreground = foreground;
                }
                SetForegroundWindow(active.hwnd);
                if let Some(button) = active.buttons.get(active.model.focused()) {
                    SetFocus(*button);
                }
            }
            return;
        }
        if let Some(outcome) = active.model.shortcut(shortcut) {
            self.finish(outcome);
        }
    }

    /// Tab, Enter, Space and Esc while the prompt has keyboard focus. The
    /// prompt model decides, so the keys behave as tested.
    fn intercept_key(&mut self, msg: &MSG) -> bool {
        if msg.message != WM_KEYDOWN {
            return false;
        }
        let Some(active) = &mut self.active else {
            return false;
        };
        // SAFETY: handle comparison and a key-state query.
        let ours = unsafe { msg.hwnd == active.hwnd || IsChild(active.hwnd, msg.hwnd) != 0 };
        if !ours {
            return false;
        }
        let key = match msg.wParam as u16 {
            VK_TAB => {
                // SAFETY: key-state query.
                if unsafe { GetKeyState(VK_SHIFT as i32) } < 0 {
                    Key::ShiftTab
                } else {
                    Key::Tab
                }
            }
            VK_RETURN => Key::Enter,
            VK_SPACE => Key::Space,
            VK_ESCAPE => Key::Escape,
            _ => return false,
        };
        match active.model.key(key) {
            Some(outcome) => self.finish(outcome),
            None => {
                if let Some(button) = active.buttons.get(active.model.focused()) {
                    // SAFETY: our own child window.
                    unsafe { SetFocus(*button) };
                }
            }
        }
        true
    }

    fn background_brush(&self) -> Option<HBRUSH> {
        self.active.as_ref().map(|a| a.background)
    }

    fn border_brush(&self) -> Option<HBRUSH> {
        self.active.as_ref().map(|a| a.border)
    }

    fn control_colors(&self) -> Option<(u32, u32, HBRUSH)> {
        self.active
            .as_ref()
            .map(|a| (a.palette.text, a.palette.background, a.background))
    }
}

/// A UI font `px` physical pixels high.
unsafe fn make_font(px: f32, weight: i32) -> HFONT {
    let face = wide("Segoe UI");
    CreateFontW(
        -(px.round() as i32),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        1, // DEFAULT_CHARSET
        0,
        0,
        5, // CLEARTYPE_QUALITY
        0,
        face.as_ptr(),
    )
}

/// Lines `text` needs when word-wrapped to `width` pixels in `font`.
unsafe fn count_lines(font: HFONT, text: &str, width: i32, scale: f32) -> u32 {
    let hdc = GetDC(null_mut());
    let old = SelectObject(hdc, font as _);
    let mut r = RECT {
        left: 0,
        top: 0,
        right: width.max(1),
        bottom: 0,
    };
    let w = wide(text);
    DrawTextW(
        hdc,
        w.as_ptr(),
        -1,
        &mut r,
        DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
    );
    SelectObject(hdc, old);
    ReleaseDC(null_mut(), hdc);
    let line = (size::BODY_LINE * scale).round().max(1.0) as i32;
    ((r.bottom - r.top + line - 1) / line).max(1) as u32
}

/// The `ScreenRect` a window occupies, for tests.
pub fn window_rect(hwnd: isize) -> Option<ScreenRect> {
    let mut r: RECT = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: out-pointer to a RECT; a stale handle only makes it fail.
    let ok = unsafe { GetWindowRect(hwnd as HWND, &mut r) };
    (ok != 0).then_some(ScreenRect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    })
}

/// The keyboard hint for settings and the about page.
pub fn keys_hint() -> &'static str {
    tingskriver_core::strings::PROMPT_KEYS_HINT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    #[test]
    fn reentrant_actions_wait_for_the_loan_and_drain_in_order() {
        let (_tx, rx) = mpsc::channel();
        let (events, _rx) = mpsc::channel();
        UI.with(|ui| *ui.borrow_mut() = Some(Ui::new(null_mut(), rx, events)));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let recorded = calls.clone();
        with_ui(|_| {
            ui_action(move |_| {
                recorded.borrow_mut().push(1);
                ui_action(move |_| recorded.borrow_mut().push(3));
            });
            let recorded = calls.clone();
            ui_action(move |_| recorded.borrow_mut().push(2));
            assert!(calls.borrow().is_empty());
        });
        assert_eq!(*calls.borrow(), [1, 2, 3]);
        UI.with(|ui| ui.borrow_mut().take());
    }

    #[test]
    fn activation_during_open_is_applied_after_the_prompt_is_installed() {
        let host = create_host().expect("host window");
        let (_tx, rx) = mpsc::channel();
        let (events, _rx) = mpsc::channel();
        UI.with(|ui| *ui.borrow_mut() = Some(Ui::new(host, rx, events)));
        let hwnd = Cell::new(null_mut());
        with_ui(|ui| {
            let active = ui
                .open(
                    PromptRequest::meeting_end(1, Duration::from_secs(60)),
                    Instant::now(),
                )
                .expect("prompt window");
            hwnd.set(active.hwnd);
            // Emulate the synchronous activation notification while open's
            // result has not yet been installed in Ui.
            unsafe { prompt_proc(active.hwnd, WM_ACTIVATE, 1, 0) };
            assert!(!active.has_keyboard_focus);
            ui.active = Some(active);
        });
        with_ui(|ui| {
            let active = ui.active.as_ref().unwrap();
            assert!(active.has_keyboard_focus);
            assert!(active.model.is_paused());
        });
        unsafe { prompt_proc(hwnd.get(), WM_ACTIVATE, WA_INACTIVE as usize, 0) };
        with_ui(|ui| {
            let active = ui.active.as_ref().unwrap();
            assert!(!active.has_keyboard_focus);
            assert!(!active.model.is_paused());
        });
        UI.with(|ui| ui.borrow_mut().take());
        unsafe { DestroyWindow(host) };
    }
}
