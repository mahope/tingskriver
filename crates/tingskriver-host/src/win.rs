//! The host thread: a message-only window with its own Win32 message loop,
//! outside GPUI, that owns the tray icon and the `RegisterHotKey` shortcuts.
//!
//! Other threads talk to it by queueing a [`Command`] and posting
//! `WM_HOST_COMMAND` to the window. Posting to a window (not the thread)
//! keeps the message when a modal loop runs, such as the open tray menu.
//! `WM_HOTKEY` only carries the id of the shortcut that fired, never a key.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use tingskriver_core::shortcuts::{Conflict, Registration};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_CLASS_ALREADY_EXISTS, ERROR_HOTKEY_ALREADY_REGISTERED, HWND, LPARAM,
    LRESULT, WPARAM,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Power::{
    RegisterSuspendResumeNotification, UnregisterSuspendResumeNotification, HPOWERNOTIFY,
};
use windows_sys::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    PostQuitMessage, RegisterClassExW, TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, HWND_MESSAGE,
    MSG, WM_APP, WM_CLOSE, WM_DESTROY, WM_HOTKEY, WNDCLASSEXW, WNDPROC,
};

use crate::hotkeys::{Hotkeys, RegisterError, Registrar};
use crate::icon;
use crate::power::{self, PowerSink};
use crate::status::{IconState, MenuEntry, TrayStatus};
use crate::watch::Wanted;
use crate::Trigger;

const WM_HOST_COMMAND: u32 = WM_APP + 0x31;

enum Command {
    Status(TrayStatus),
    Shortcuts(Wanted, tokio::sync::oneshot::Sender<Vec<Conflict>>),
}

type Sink = Arc<dyn Fn(Trigger) + Send + Sync>;

/// The running host thread. Dropping it removes the tray icon and the
/// shortcuts and ends the thread.
pub(crate) struct HostThread {
    hwnd: isize,
    has_tray: bool,
    commands: std::sync::Mutex<Sender<Command>>,
    thread: Option<JoinHandle<()>>,
}

impl HostThread {
    pub fn start(sink: Sink, power: PowerSink) -> Result<HostThread, String> {
        let (tx, rx) = channel();
        let (ready_tx, ready_rx) = channel::<Result<(isize, bool), String>>();
        let thread = std::thread::Builder::new()
            .name("tingskriver-host".into())
            .spawn(move || host_thread(rx, sink, power, ready_tx))
            .map_err(|e| format!("could not start the host thread: {e}"))?;
        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok((hwnd, has_tray))) => Ok(HostThread {
                hwnd,
                has_tray,
                commands: std::sync::Mutex::new(tx),
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the host thread did not start".into()),
        }
    }

    fn send(&self, command: Command) {
        let sent = self
            .commands
            .lock()
            .map(|tx| tx.send(command).is_ok())
            .unwrap_or(false);
        if sent {
            // SAFETY: posting to a window this process owns; a stale handle only fails.
            unsafe { PostMessageW(self.hwnd as HWND, WM_HOST_COMMAND, 0, 0) };
        }
    }

    pub fn has_tray(&self) -> bool {
        self.has_tray
    }

    pub fn set_status(&self, status: TrayStatus) {
        self.send(Command::Status(status));
    }

    pub async fn apply_shortcuts(&self, wanted: Wanted) -> Vec<Conflict> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.send(Command::Shortcuts(wanted, reply));
        answer.await.unwrap_or_default()
    }
}

impl Drop for HostThread {
    fn drop(&mut self) {
        // SAFETY: see `send`.
        unsafe { PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Executable names of the running processes.
pub(crate) fn process_names() -> Vec<String> {
    let mut names = Vec::new();
    // SAFETY: a process snapshot read with a correctly sized entry, then closed.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return names;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
            ok = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    names
}

struct Win32Registrar {
    hwnd: HWND,
}

impl Registrar for Win32Registrar {
    fn register(&mut self, id: i32, r: Registration) -> Result<(), RegisterError> {
        // SAFETY: the window belongs to this thread.
        if unsafe { RegisterHotKey(self.hwnd, id, r.modifiers, r.vk) } != 0 {
            return Ok(());
        }
        match unsafe { GetLastError() } {
            ERROR_HOTKEY_ALREADY_REGISTERED => Err(RegisterError::Taken),
            code => Err(RegisterError::Other(format!("fejl {code}"))),
        }
    }

    fn unregister(&mut self, id: i32) {
        // SAFETY: as above; unregistering an unknown id only fails.
        unsafe { UnregisterHotKey(self.hwnd, id) };
    }
}

struct State {
    commands: Receiver<Command>,
    sink: Sink,
    tray: Option<TrayIcon>,
    items: Vec<(MenuEntry, MenuItem)>,
    icon: Option<IconState>,
    tooltip: String,
    hotkeys: Hotkeys<Win32Registrar>,
    session: Option<PowerWindow>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static POWER: RefCell<Option<PowerSink>> = const { RefCell::new(None) };
}

/// A hidden top-level window for sleep, lock and logoff (issue #179).
/// `WM_QUERYENDSESSION` and `WM_ENDSESSION` are broadcast to top-level
/// windows only, so the message-only host window cannot get them.
struct PowerWindow {
    hwnd: HWND,
    suspend: HPOWERNOTIFY,
    sessions: bool,
}

impl PowerWindow {
    fn create(sink: PowerSink) -> Option<PowerWindow> {
        POWER.with(|p| *p.borrow_mut() = Some(sink));
        let hwnd =
            match create_hidden_window("TingskriverStroem\0", null_mut(), Some(power_wndproc)) {
                Ok(hwnd) => hwnd,
                Err(e) => {
                    log::error!("power: window for sleep and logoff could not be created: {e}");
                    return None;
                }
            };
        // SAFETY: the window belongs to this thread; both registrations are
        // undone in `Drop`.
        let (suspend, sessions) = unsafe {
            (
                RegisterSuspendResumeNotification(hwnd, DEVICE_NOTIFY_WINDOW_HANDLE),
                WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) != 0,
            )
        };
        if suspend == 0 {
            log::error!(
                "power: sleep notifications could not be registered: {}",
                unsafe { GetLastError() }
            );
        }
        if !sessions {
            log::warn!("power: lock and unlock notifications could not be registered");
        }
        log::info!("power: listening for sleep, resume, lock, unlock and logoff");
        Some(PowerWindow {
            hwnd,
            suspend,
            sessions,
        })
    }
}

impl Drop for PowerWindow {
    fn drop(&mut self) {
        // SAFETY: undoing what `create` did, on the same thread.
        unsafe {
            if self.suspend != 0 {
                UnregisterSuspendResumeNotification(self.suspend);
            }
            if self.sessions {
                WTSUnRegisterSessionNotification(self.hwnd);
            }
            DestroyWindow(self.hwnd);
        }
        POWER.with(|p| p.borrow_mut().take());
    }
}

/// Messages for the power window. The sink saves the recording before it
/// returns; Windows waits for this function, so that is what keeps the
/// machine awake long enough.
unsafe extern "system" fn power_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let Some(event) = power::event_from_message(msg, wparam) else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };
    // Clone first, so the borrow is over while the sink works.
    if let Some(sink) = POWER.with(|p| p.borrow().clone()) {
        sink(event);
    }
    match msg {
        // Never veto a logoff or sleep; TRUE lets it go ahead.
        power::WM_QUERYENDSESSION | power::WM_POWERBROADCAST => 1,
        _ => 0,
    }
}

fn menu_id(entry: MenuEntry) -> &'static str {
    match entry {
        MenuEntry::NewRecording => "tingskriver.new-recording",
        MenuEntry::Dictate => "tingskriver.dictate",
        MenuEntry::Ask => "tingskriver.ask",
        MenuEntry::Open => "tingskriver.open",
        MenuEntry::Quit => "tingskriver.quit",
    }
}

fn tray_icon(state: IconState) -> Option<Icon> {
    match Icon::from_rgba(icon::rgba(state), icon::SIZE, icon::SIZE) {
        Ok(icon) => Some(icon),
        Err(e) => {
            log::warn!("Tray icon could not be made: {e}");
            None
        }
    }
}

fn host_thread(
    commands: Receiver<Command>,
    sink: Sink,
    power: PowerSink,
    ready: Sender<Result<(isize, bool), String>>,
) {
    let hwnd = match create_window() {
        Ok(hwnd) => hwnd,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    // The handlers are global (set once per process) and run on this thread.
    {
        let menu_sink = sink.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(entry) = MenuEntry::ALL
                .into_iter()
                .find(|e| event.id.0 == menu_id(*e))
            {
                menu_sink(Trigger::Menu(entry));
            }
        }));
        let click_sink = sink.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                click_sink(Trigger::TrayClicked);
            }
        }));
    }

    let status = TrayStatus::default();
    let menu = Menu::new();
    let mut items = Vec::new();
    for entry in MenuEntry::ALL {
        if entry == MenuEntry::Open {
            let _ = menu.append(&PredefinedMenuItem::separator());
        }
        let label = status.menu(entry);
        let item = MenuItem::with_id(menu_id(entry), &label.text, label.enabled, None);
        let _ = menu.append(&item);
        items.push((entry, item));
    }
    let tooltip = status.tooltip();
    let mut builder = TrayIconBuilder::new()
        .with_id("tingskriver")
        .with_tooltip(&tooltip)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false);
    if let Some(icon) = tray_icon(status.icon()) {
        builder = builder.with_icon(icon);
    }
    let tray = match builder.build() {
        Ok(tray) => Some(tray),
        Err(e) => {
            // Shortcuts still work without a tray icon.
            log::error!("Tray icon could not be created: {e}");
            None
        }
    };

    let has_tray = tray.is_some();
    let session = PowerWindow::create(power);
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            commands,
            sink,
            tray,
            items,
            icon: Some(status.icon()),
            tooltip,
            hotkeys: Hotkeys::new(Win32Registrar { hwnd }),
            session,
        })
    });
    let _ = ready.send(Ok((hwnd as isize, has_tray)));
    log::info!("Host thread running: tray and shortcuts");

    // SAFETY: the standard message loop of the thread that owns the window.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    STATE.with(|s| s.borrow_mut().take());
    log::info!("Host thread stopped");
}

fn create_window() -> Result<HWND, String> {
    create_hidden_window("TingskriverVaert\0", HWND_MESSAGE, Some(wndproc))
}

/// A hidden window of `class_name` (NUL-terminated); `parent` is
/// `HWND_MESSAGE` for a message-only window, null for a top-level one.
fn create_hidden_window(class_name: &str, parent: HWND, proc: WNDPROC) -> Result<HWND, String> {
    let class: Vec<u16> = class_name.encode_utf16().collect();
    // SAFETY: the class name outlives the calls; `proc` has the right signature.
    unsafe {
        let instance = GetModuleHandleW(null());
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = proc;
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        // A second window of the same class finds it registered, which is
        // fine; any other failure is real and would only show up later as a
        // window that cannot be created.
        if RegisterClassExW(&wc) == 0 {
            let error = GetLastError();
            if error != ERROR_CLASS_ALREADY_EXISTS {
                return Err(format!("RegisterClassExW failed: error {error}"));
            }
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            parent,
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return Err(format!("CreateWindowExW failed: {}", GetLastError()));
        }
        Ok(hwnd)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            // Look up first and call the sink after the borrow ends.
            let fired = STATE.with(|s| {
                s.borrow().as_ref().and_then(|state| {
                    Some((state.hotkeys.action(wparam as i32)?, state.sink.clone()))
                })
            });
            if let Some((action, sink)) = fired {
                sink(Trigger::Shortcut(action));
            }
            0
        }
        WM_HOST_COMMAND => {
            STATE.with(|s| {
                if let Some(state) = s.borrow_mut().as_mut() {
                    state.run_commands();
                }
            });
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            STATE.with(|s| {
                if let Some(state) = s.borrow_mut().as_mut() {
                    state.hotkeys.clear();
                    // Unregisters the sleep notifications and destroys its window.
                    state.session.take();
                    // Remove the icon now, not when the thread's locals go.
                    state.tray.take();
                }
            });
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl State {
    fn run_commands(&mut self) {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                Command::Status(status) => self.show(&status),
                Command::Shortcuts(wanted, reply) => {
                    let problems = self
                        .hotkeys
                        .apply(wanted.dictation.as_deref(), &wanted.settings);
                    let _ = reply.send(problems);
                }
            }
        }
    }

    fn show(&mut self, status: &TrayStatus) {
        let Some(tray) = &self.tray else {
            return;
        };
        let state = status.icon();
        if self.icon != Some(state) {
            if let Err(e) = tray.set_icon(tray_icon(state)) {
                log::warn!("Tray icon could not be changed: {e}");
            }
            self.icon = Some(state);
        }
        let tooltip = status.tooltip();
        if tooltip != self.tooltip {
            let _ = tray.set_tooltip(Some(&tooltip));
            self.tooltip = tooltip;
        }
        for (entry, item) in &self.items {
            let label = status.menu(*entry);
            if item.text() != label.text {
                item.set_text(&label.text);
            }
            if item.is_enabled() != label.enabled {
                item.set_enabled(label.enabled);
            }
        }
    }
}

/// Registration against the real system: the shortcut a test registers is
/// taken for everyone else until it is unregistered.
#[cfg(test)]
mod tests {
    use super::*;
    use tingskriver_core::dictation::logic::hotkey::Chord;
    use tingskriver_core::shortcuts::{self, ConflictKind, ShortcutSettings};

    fn registration(text: &str) -> Registration {
        shortcuts::registration(Chord::parse(text).unwrap()).unwrap()
    }

    #[test]
    fn lists_running_processes() {
        let names = process_names();
        let own = std::env::current_exe().unwrap();
        let own = own.file_name().unwrap().to_string_lossy();
        assert!(
            names.iter().any(|n| n.eq_ignore_ascii_case(&own)),
            "{own} not in {} processes",
            names.len()
        );
    }

    #[test]
    fn host_thread_registers_and_swaps_shortcuts_while_running() {
        let thread = HostThread::start(Arc::new(|_| {}), Arc::new(|_| {})).expect("host thread");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // Unusual keys, so the test never collides with a real program.
        let first = ShortcutSettings {
            open_window: Some("Ctrl+Alt+Shift+F23".into()),
            ..ShortcutSettings::default()
        };
        let problems = runtime.block_on(thread.apply_shortcuts(Wanted {
            dictation: None,
            settings: first,
        }));
        assert!(problems.is_empty(), "{problems:?}");

        // Another window (as another program would) cannot take it now.
        let other = create_window().unwrap();
        let mut probe = Win32Registrar { hwnd: other };
        assert_eq!(
            probe.register(77, registration("Ctrl+Alt+Shift+F23")),
            Err(RegisterError::Taken)
        );

        // Change it while running: the old one is free, the new one taken.
        let second = ShortcutSettings {
            open_window: Some("Ctrl+Alt+Shift+F24".into()),
            ..ShortcutSettings::default()
        };
        let problems = runtime.block_on(thread.apply_shortcuts(Wanted {
            dictation: None,
            settings: second,
        }));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            probe.register(77, registration("Ctrl+Alt+Shift+F23")),
            Ok(())
        );
        probe.unregister(77);
        assert_eq!(
            probe.register(78, registration("Ctrl+Alt+Shift+F24")),
            Err(RegisterError::Taken)
        );

        // A shortcut another program holds is reported, not silently lost.
        assert_eq!(
            probe.register(79, registration("Ctrl+Alt+Shift+F22")),
            Ok(())
        );
        let problems = runtime.block_on(thread.apply_shortcuts(Wanted {
            dictation: None,
            settings: ShortcutSettings {
                ask: Some("Ctrl+Alt+Shift+F22".into()),
                ..ShortcutSettings::default()
            },
        }));
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].kind, ConflictKind::TakenByOtherProgram);
        probe.unregister(79);
        unsafe { DestroyWindow(other) };

        // Dropping the host frees every shortcut.
        drop(thread);
        let other = create_window().unwrap();
        let mut probe = Win32Registrar { hwnd: other };
        assert_eq!(
            probe.register(80, registration("Ctrl+Alt+Shift+F24")),
            Ok(())
        );
        probe.unregister(80);
        unsafe { DestroyWindow(other) };
    }
}
