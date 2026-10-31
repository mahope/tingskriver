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
use std::sync::{Arc, Once};
use std::thread::JoinHandle;
use std::time::Duration;

use tingskriver_core::shortcuts::{Conflict, Registration};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_HOTKEY_ALREADY_REGISTERED, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    PostQuitMessage, RegisterClassExW, TranslateMessage, HWND_MESSAGE, MSG, WM_APP, WM_CLOSE,
    WM_DESTROY, WM_HOTKEY, WNDCLASSEXW,
};

use crate::hotkeys::{Hotkeys, RegisterError, Registrar};
use crate::icon;
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
    pub fn start(sink: Sink) -> Result<HostThread, String> {
        let (tx, rx) = channel();
        let (ready_tx, ready_rx) = channel::<Result<(isize, bool), String>>();
        let thread = std::thread::Builder::new()
            .name("tingskriver-host".into())
            .spawn(move || host_thread(rx, sink, ready_tx))
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
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
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
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            commands,
            sink,
            tray,
            items,
            icon: Some(status.icon()),
            tooltip,
            hotkeys: Hotkeys::new(Win32Registrar { hwnd }),
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
    static REGISTER: Once = Once::new();
    let class: Vec<u16> = "TingskriverVaert\0".encode_utf16().collect();
    // SAFETY: the class name outlives the calls; `wndproc` has the right signature.
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
        let hwnd = CreateWindowExW(
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
    fn host_thread_registers_and_swaps_shortcuts_while_running() {
        let thread = HostThread::start(Arc::new(|_| {})).expect("host thread");
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
