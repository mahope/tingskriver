//! The small pill at the bottom of the screen while dictating.
//!
//! A borderless, transparent, always-on-top window that never takes focus
//! (on Windows it is a click-through tool window with `WS_EX_NOACTIVATE`).
//! State is pushed with `eval`, so the page needs no IPC permissions. The
//! dictation service in core drives it through [`TauriOverlay`].

// The shortcut, overlay and insertion only run on Windows.
#![cfg_attr(not(windows), allow(dead_code))]

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tingskriver_core::dictation::overlay::{Overlay, OverlayState};

pub const LABEL: &str = "dictation-overlay";
const WIDTH: i32 = 220;
const HEIGHT: i32 = 52;
const MARGIN: i32 = 64;

fn window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(LABEL)
}

/// Create the (hidden) overlay window if it does not exist yet.
pub fn ensure<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if window(app).is_some() {
        return Ok(());
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("dictation-overlay.html".into()))
        .title("Tingskriver diktering")
        .inner_size(WIDTH as f64, HEIGHT as f64)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()
        .map_err(|e| format!("could not create dictation overlay: {e}"))?;

    #[cfg(windows)]
    if let Ok(hwnd) = built.hwnd() {
        super::win::make_overlay_window(hwnd.0 as isize);
    }
    #[cfg(not(windows))]
    let _ = built;
    Ok(())
}

pub fn set<R: Runtime>(app: &AppHandle<R>, state: OverlayState) {
    let Some(win) = window(app) else {
        return;
    };
    let script = match state {
        OverlayState::Recording { level, locked } => format!(
            "window.__tingskriverDictation && window.__tingskriverDictation({{state:'{}',level:{:.3}}})",
            if locked { "locked" } else { "recording" },
            level.clamp(0.0, 1.0)
        ),
        OverlayState::Processing => {
            "window.__tingskriverDictation && window.__tingskriverDictation({state:'processing'})".to_string()
        }
        OverlayState::Error => {
            "window.__tingskriverDictation && window.__tingskriverDictation({state:'error'})".to_string()
        }
        OverlayState::Hidden => {
            "window.__tingskriverDictation && window.__tingskriverDictation({state:'hidden'})".to_string()
        }
    };
    let _ = win.eval(&script);

    match state {
        OverlayState::Hidden => hide(&win),
        _ => show(&win),
    }
}

#[cfg(windows)]
fn show<R: Runtime>(win: &WebviewWindow<R>) {
    if let Ok(hwnd) = win.hwnd() {
        super::win::show_overlay(hwnd.0 as isize, WIDTH, HEIGHT, MARGIN);
    }
}

#[cfg(windows)]
fn hide<R: Runtime>(win: &WebviewWindow<R>) {
    if let Ok(hwnd) = win.hwnd() {
        super::win::hide_overlay(hwnd.0 as isize);
    }
}

#[cfg(not(windows))]
fn show<R: Runtime>(win: &WebviewWindow<R>) {
    let _ = win.show();
}

#[cfg(not(windows))]
fn hide<R: Runtime>(win: &WebviewWindow<R>) {
    let _ = win.hide();
}

/// The webview overlay as the core dictation service sees it.
pub struct TauriOverlay<R: Runtime>(pub AppHandle<R>);

impl<R: Runtime> Overlay for TauriOverlay<R> {
    fn ensure(&self) -> Result<(), String> {
        ensure(&self.0)
    }

    fn set(&self, state: OverlayState) {
        set(&self.0, state)
    }
}
