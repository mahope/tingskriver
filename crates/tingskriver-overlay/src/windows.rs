//! The pill as a native Win32 layered window.
//!
//! One hidden window is created up front on its own thread, which owns the
//! message loop. Callers on any thread send commands over a channel and wake
//! the thread with `PostMessageW`, so nothing blocks the dictation or audio
//! threads. While the pill is visible a 33 ms timer draws frames with
//! tiny-skia into a DIB that `UpdateLayeredWindow` puts on screen; when it is
//! hidden the timer is killed and the thread sleeps in `GetMessageW`.
//!
//! The window never takes focus: it is `WS_EX_NOACTIVATE` (and answers
//! `WM_MOUSEACTIVATE` with `MA_NOACTIVATE`), click-through
//! (`WS_EX_TRANSPARENT`, `HTTRANSPARENT`), shown with `SW_SHOWNOACTIVATE`
//! and kept out of the taskbar and Alt+Tab (`WS_EX_TOOLWINDOW`).

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Once};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromPoint, MonitorFromWindow, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
    MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetForegroundWindow, GetMessageW, KillTimer, PostMessageW, PostQuitMessage, RegisterClassExW,
    SetTimer, SetWindowPos, ShowWindow, SystemParametersInfoW, UpdateLayeredWindow, HTTRANSPARENT,
    HWND_TOPMOST, MA_NOACTIVATE, MSG, SPI_GETCLIENTAREAANIMATION, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_APP, WM_DESTROY, WM_DISPLAYCHANGE,
    WM_DPICHANGED, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::layout::{place, ScreenRect};
use crate::model::{Change, Pill, PillState};
use crate::render::{to_bgra, Renderer};
use crate::theme::timing;

const WM_WAKE: u32 = WM_APP + 1;
const FRAME_TIMER: usize = 1;
/// The state-change-to-screen budget from the issue.
pub const SHOW_BUDGET: Duration = Duration::from_millis(50);

enum Command {
    State(PillState, Instant),
    Level(f32),
    Show(Instant),
    Hide,
    Quit,
}

/// Counters the window thread keeps, for tests, the demo and logging.
#[derive(Default)]
struct Counters {
    frames: AtomicU64,
    shows: AtomicU64,
    last_show_us: AtomicU64,
    max_show_us: AtomicU64,
}

/// A snapshot of the overlay's counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    /// Frames drawn since the overlay was created.
    pub frames: u64,
    /// Times the hidden pill was shown.
    pub shows: u64,
    /// From the call that showed the pill to its first frame on screen.
    pub last_show_latency: Option<Duration>,
    pub max_show_latency: Option<Duration>,
}

/// The pill window. Cheap to call from any thread; drawing happens on the
/// overlay's own thread. Dropping it destroys the window.
pub struct Overlay {
    tx: Sender<Command>,
    hwnd: isize,
    counters: Arc<Counters>,
    thread: Option<JoinHandle<()>>,
}

impl Overlay {
    /// Create the (hidden) window and its thread.
    pub fn new() -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let counters = Arc::new(Counters::default());
        let thread_counters = counters.clone();
        let thread = std::thread::Builder::new()
            .name("tingskriver-overlay".into())
            .spawn(move || ui_thread(rx, ready_tx, thread_counters))
            .map_err(|e| format!("could not start the overlay thread: {e}"))?;
        let hwnd = ready_rx
            .recv()
            .map_err(|_| "the overlay thread stopped before creating its window".to_string())??;
        Ok(Self {
            tx,
            hwnd,
            counters,
            thread: Some(thread),
        })
    }

    fn send(&self, command: Command) {
        if self.tx.send(command).is_ok() {
            // SAFETY: posting to a window handle is safe even if it is gone.
            unsafe { PostMessageW(self.hwnd as HWND, WM_WAKE, 0, 0) };
        }
    }

    /// Change what the pill shows. Any state but [`PillState::Idle`] also
    /// shows it; `Idle` hides it.
    pub fn set_state(&self, state: PillState) {
        self.send(Command::State(state, Instant::now()));
    }

    /// Update the input level while listening (0.0..=1.0).
    pub fn set_level(&self, level: f32) {
        self.send(Command::Level(level));
    }

    /// Show the current state again after [`Overlay::hide`].
    pub fn show(&self) {
        self.send(Command::Show(Instant::now()));
    }

    /// Hide the pill; the state is kept.
    pub fn hide(&self) {
        self.send(Command::Hide);
    }

    /// The window handle, for tests and diagnostics.
    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    pub fn stats(&self) -> Stats {
        let us = |v: u64| (v > 0).then(|| Duration::from_micros(v));
        Stats {
            frames: self.counters.frames.load(Ordering::Relaxed),
            shows: self.counters.shows.load(Ordering::Relaxed),
            last_show_latency: us(self.counters.last_show_us.load(Ordering::Relaxed)),
            max_show_latency: us(self.counters.max_show_us.load(Ordering::Relaxed)),
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        self.send(Command::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The pill for core's dictation service. The window is created on the
/// first [`ensure`](tingskriver_core::dictation::overlay::Overlay::ensure).
#[derive(Default)]
pub struct CoreOverlay {
    overlay: Mutex<Option<Overlay>>,
}

impl CoreOverlay {
    pub fn new() -> Self {
        Self::default()
    }
}

impl tingskriver_core::dictation::overlay::Overlay for CoreOverlay {
    fn ensure(&self) -> Result<(), String> {
        let mut slot = self
            .overlay
            .lock()
            .map_err(|_| "overlay lock poisoned".to_string())?;
        if slot.is_none() {
            *slot = Some(Overlay::new()?);
        }
        Ok(())
    }

    fn set(&self, state: tingskriver_core::dictation::overlay::OverlayState) {
        if let Err(e) = self.ensure() {
            log::warn!("Overlay: {e}");
            return;
        }
        if let Ok(slot) = self.overlay.lock() {
            if let Some(overlay) = slot.as_ref() {
                overlay.set_state(crate::from_core(state));
            }
        }
    }
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn ui_thread(rx: Receiver<Command>, ready: Sender<Result<isize, String>>, counters: Arc<Counters>) {
    // Physical pixels and per-monitor DPI for everything this thread creates,
    // whatever the host process declared.
    // SAFETY: plain Win32 calls on this thread.
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let hwnd = match create_window() {
        Ok(hwnd) => hwnd,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    UI.with(|ui| *ui.borrow_mut() = Some(Ui::new(hwnd, rx, counters)));
    let _ = ready.send(Ok(hwnd as isize));

    // SAFETY: standard message loop on the thread that owns the window.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }
    }
    UI.with(|ui| ui.borrow_mut().take());
}

fn create_window() -> Result<HWND, String> {
    static REGISTER: Once = Once::new();
    let class = wide("TingskriverPille");
    // SAFETY: the class name and title outlive the calls; the window
    // procedure has the right signature.
    unsafe {
        let instance = GetModuleHandleW(null());
        REGISTER.call_once(|| {
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = instance;
            wc.lpszClassName = class.as_ptr();
            RegisterClassExW(&wc);
        });
        let title = wide("Tingskriver diktering");
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return Err(format!(
                "could not create the overlay window: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(hwnd)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_WAKE => {
            with_ui(Ui::drain);
            0
        }
        WM_TIMER if wparam == FRAME_TIMER => {
            with_ui(Ui::on_timer);
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        // The pill is re-laid out for the monitor's DPI when it is shown;
        // the suggested window rectangle does not apply to a layered pill.
        WM_DPICHANGED => 0,
        WM_DISPLAYCHANGE => {
            with_ui(Ui::on_display_change);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn with_ui(f: impl FnOnce(&mut Ui)) {
    UI.with(|ui| {
        // Win32 can re-enter the window procedure from inside a call we make;
        // that nested message is skipped instead of panicking.
        if let Ok(mut ui) = ui.try_borrow_mut() {
            if let Some(ui) = ui.as_mut() {
                f(ui);
            }
        }
    });
}

/// The monitor the pill is on, chosen when it is shown.
#[derive(Clone, Copy)]
struct Monitor {
    work: ScreenRect,
    scale: f32,
}

struct Ui {
    hwnd: HWND,
    rx: Receiver<Command>,
    counters: Arc<Counters>,
    pill: Pill,
    renderer: Renderer,
    surface: Option<Surface>,
    monitor: Option<Monitor>,
    window_shown: bool,
    timer_on: bool,
}

impl Ui {
    fn new(hwnd: HWND, rx: Receiver<Command>, counters: Arc<Counters>) -> Self {
        Self {
            hwnd,
            rx,
            counters,
            pill: Pill::new(Instant::now()),
            renderer: Renderer::new(),
            surface: None,
            monitor: None,
            window_shown: false,
            timer_on: false,
        }
    }

    fn drain(&mut self) {
        let mut change = Change::None;
        let mut requested_at: Option<Instant> = None;
        while let Ok(command) = self.rx.try_recv() {
            let now = Instant::now();
            let (c, at) = match command {
                Command::State(state, at) => (self.pill.set_state(state, now), Some(at)),
                Command::Level(level) => {
                    self.pill.set_level(level);
                    (Change::None, None)
                }
                Command::Show(at) => (self.pill.show(now), Some(at)),
                Command::Hide => (self.pill.hide(now), None),
                Command::Quit => {
                    self.stop_timer();
                    // SAFETY: our own window, on its thread.
                    unsafe { DestroyWindow(self.hwnd) };
                    return;
                }
            };
            if c == Change::Show {
                requested_at = requested_at.or(at);
            }
            change = merge(change, c);
        }
        match change {
            Change::Show => {
                self.monitor = Some(current_monitor());
                self.pill.set_reduced_motion(!animations_enabled());
                self.present(requested_at);
            }
            Change::Redraw => self.present(None),
            Change::None => {}
        }
        self.update_timer();
    }

    fn on_timer(&mut self) {
        self.pill.tick(Instant::now());
        self.present(None);
        self.update_timer();
    }

    fn on_display_change(&mut self) {
        if self.window_shown {
            self.monitor = Some(current_monitor());
            self.present(None);
        }
    }

    /// Draw the current frame, or hide the window when there is none.
    fn present(&mut self, requested_at: Option<Instant>) {
        let now = Instant::now();
        let Some(frame) = self.pill.frame(now) else {
            self.hide_window();
            return;
        };
        let monitor = self.monitor.unwrap_or_else(current_monitor);
        let (pixmap, layout) = self.renderer.render(&frame, monitor.scale);
        let mut surface = match self.surface.take() {
            Some(s) if s.width == layout.width && s.height == layout.height => s,
            _ => match Surface::new(layout.width, layout.height) {
                Some(s) => s,
                None => {
                    log::warn!(
                        "Overlay: could not create a {}x{} bitmap",
                        layout.width,
                        layout.height
                    );
                    return;
                }
            },
        };
        to_bgra(&pixmap, surface.pixels());
        let (x, y) = place(monitor.work, layout.width, layout.height, monitor.scale);
        let position = POINT { x, y };
        let size = SIZE {
            cx: layout.width as i32,
            cy: layout.height as i32,
        };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (frame.opacity * 255.0).round() as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        // SAFETY: the DC, bitmap and window are alive; pointers are to locals.
        let ok = unsafe {
            let screen = GetDC(null_mut());
            let ok = UpdateLayeredWindow(
                self.hwnd, screen, &position, &size, surface.dc, &origin, 0, &blend, ULW_ALPHA,
            );
            ReleaseDC(null_mut(), screen);
            ok != 0
        };
        self.surface = Some(surface);
        if !ok {
            log::warn!(
                "Overlay: UpdateLayeredWindow failed: {}",
                std::io::Error::last_os_error()
            );
            return;
        }
        if !self.window_shown {
            // SAFETY: our own window, on its thread.
            unsafe {
                ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                // Above topmost windows that came up after ours.
                SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            self.window_shown = true;
        }
        self.counters.frames.fetch_add(1, Ordering::Relaxed);
        if let Some(at) = requested_at {
            let latency = at.elapsed();
            let us = (latency.as_micros() as u64).max(1);
            self.counters.shows.fetch_add(1, Ordering::Relaxed);
            self.counters.last_show_us.store(us, Ordering::Relaxed);
            self.counters.max_show_us.fetch_max(us, Ordering::Relaxed);
            if latency > SHOW_BUDGET {
                log::warn!("Overlay: pill took {} ms to show", latency.as_millis());
            } else {
                log::debug!("Overlay: pill shown in {} µs", us);
            }
        }
    }

    fn hide_window(&mut self) {
        if self.window_shown {
            // SAFETY: our own window, on its thread.
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
            self.window_shown = false;
        }
        // Free the bitmap while hidden; it is cheap to make again.
        self.surface = None;
    }

    /// Frames only while visible: start or kill the timer to match.
    fn update_timer(&mut self) {
        if self.pill.needs_frames() && !self.timer_on {
            // SAFETY: our own window, on its thread.
            unsafe { SetTimer(self.hwnd, FRAME_TIMER, timing::FRAME_MS as u32, None) };
            self.timer_on = true;
        } else if !self.pill.needs_frames() {
            self.stop_timer();
            self.hide_window();
        }
    }

    fn stop_timer(&mut self) {
        if self.timer_on {
            // SAFETY: our own window and timer id.
            unsafe { KillTimer(self.hwnd, FRAME_TIMER) };
            self.timer_on = false;
        }
    }
}

fn merge(a: Change, b: Change) -> Change {
    match (a, b) {
        (Change::Show, _) | (_, Change::Show) => Change::Show,
        (Change::Redraw, _) | (_, Change::Redraw) => Change::Redraw,
        _ => Change::None,
    }
}

/// The monitor with the active window (where the user is typing), or the
/// one under the mouse when no window is active, with its DPI scale.
fn current_monitor() -> Monitor {
    // SAFETY: plain Win32 queries with valid out-pointers.
    unsafe {
        let foreground = GetForegroundWindow();
        let monitor = if foreground.is_null() {
            let mut cursor = POINT { x: 0, y: 0 };
            GetCursorPos(&mut cursor);
            MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST)
        } else {
            MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST)
        };
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let work = if GetMonitorInfoW(monitor, &mut info) != 0 {
            rect(info.rcWork)
        } else {
            ScreenRect {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720,
            }
        };
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) < 0 || dpi_x == 0 {
            dpi_x = 96;
        }
        Monitor {
            work,
            scale: dpi_x as f32 / 96.0,
        }
    }
}

fn rect(r: RECT) -> ScreenRect {
    ScreenRect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

/// Windows' "Vis animationer i Windows" setting.
fn animations_enabled() -> bool {
    let mut enabled: i32 = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            &mut enabled as *mut i32 as *mut core::ffi::c_void,
            0,
        )
    };
    ok == 0 || enabled != 0
}

/// A 32-bit top-down DIB selected into a memory DC.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}

impl Surface {
    fn new(width: u32, height: u32) -> Option<Self> {
        // SAFETY: the header describes the bitmap; every handle is checked
        // and released on failure.
        unsafe {
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return None;
            }
            let mut bits: *mut core::ffi::c_void = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let previous = SelectObject(dc, bitmap);
            Some(Self {
                dc,
                bitmap,
                previous,
                bits: bits as *mut u8,
                width,
                height,
            })
        }
    }

    fn pixels(&mut self) -> &mut [u8] {
        // SAFETY: the DIB holds width * height 32-bit pixels and lives as
        // long as self; only the overlay thread touches it.
        unsafe {
            std::slice::from_raw_parts_mut(self.bits, (self.width * self.height * 4) as usize)
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: handles created in Surface::new and not freed elsewhere.
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
