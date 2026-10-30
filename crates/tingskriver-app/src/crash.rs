//! Graphics failures must never leave a silent, frozen window.
//!
//! GPUI's DirectX backend picks the first adapter that supports Direct3D 11;
//! on a machine without a usable GPU that is Microsoft's software adapter
//! (WARP, "Microsoft Basic Render Driver"), so the app renders in software.
//! When even that fails, GPUI panics ("failed to initialize DirectX"). When
//! the GPU is lost while running, GPUI rebuilds its renderer up to five
//! times and then panics ("Device lost"). This hook turns those panics into
//! a log entry and a native error dialog with the log path, and offers to
//! start the app again. Other panics are only logged: tokio catches panics
//! in core tasks, and they must not take the whole app down.

use std::path::{Path, PathBuf};

/// What a panic message says about the graphics stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashKind {
    /// No adapter could be used, not even software rendering.
    GraphicsUnavailable,
    /// The GPU disappeared (driver update, reset, eGPU unplugged) and could
    /// not be recovered.
    GraphicsLost,
    /// Anything else.
    Other,
}

pub fn classify(message: &str) -> CrashKind {
    let lower = message.to_lowercase();
    if lower.contains("failed to initialize directx") || lower.contains("creating directx devices") {
        CrashKind::GraphicsUnavailable
    } else if lower.contains("device lost") || lower.contains("lost device") {
        CrashKind::GraphicsLost
    } else {
        CrashKind::Other
    }
}

/// Title and text of the error dialog, in Danish.
pub fn dialog_text(kind: CrashKind, log_path: Option<&Path>) -> Option<(String, String)> {
    let log = match log_path {
        Some(path) => format!("Detaljerne står i logfilen:\n{}", path.display()),
        None => "Logfilen kunne ikke oprettes, så detaljerne står kun i konsollen.".to_string(),
    };
    let (title, what) = match kind {
        CrashKind::GraphicsUnavailable => (
            "Tingskriver kan ikke vise sit vindue",
            "Windows kunne hverken give Tingskriver adgang til grafikkortet eller til software-tegning (Direct3D 11).\n\n\
             Opdatér grafikdriveren, eller start Tingskriver på en anden maskine.",
        ),
        CrashKind::GraphicsLost => (
            "Tingskriver mistede forbindelsen til grafikkortet",
            "Grafikkortet forsvandt, mens Tingskriver kørte (fx ved en driveropdatering), og vinduet kunne ikke tegnes igen.\n\n\
             En igangværende optagelse er gemt løbende på disken.",
        ),
        CrashKind::Other => return None,
    };
    Some((
        title.to_string(),
        format!("{what}\n\n{log}\n\nVælg \"Prøv igen\" for at starte Tingskriver igen."),
    ))
}

/// Install the panic hook. Keeps the default hook (stderr, backtrace) and
/// adds logging and, for graphics failures, the dialog and exit.
pub fn install_panic_hook(log_path: Option<PathBuf>) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = panic_message(info);
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed");
        log::error!("Panic in thread '{thread}' at {location}: {message}");
        log::logger().flush();
        default_hook(info);

        let kind = classify(&message);
        if let Some((title, text)) = dialog_text(kind, log_path.as_deref()) {
            log::error!("Graphics failure ({kind:?}); showing the error dialog");
            log::logger().flush();
            if show_error_dialog(&title, &text) {
                restart();
            }
            std::process::exit(1);
        }
    }));
}

fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

fn restart() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = std::process::Command::new(exe).args(args).spawn() {
        log::error!("Could not start Tingskriver again: {error}");
    }
}

/// A modal native dialog; `true` when the user chose "Prøv igen". It must
/// not depend on GPUI, which is what just failed.
#[cfg(windows)]
fn show_error_dialog(title: &str, text: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDRETRY, MB_ICONERROR, MB_RETRYCANCEL, MB_SETFOREGROUND, MB_TOPMOST,
    };
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (title, text) = (wide(title), wide(text));
    let choice = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_ICONERROR | MB_RETRYCANCEL | MB_SETFOREGROUND | MB_TOPMOST,
        )
    };
    choice == IDRETRY
}

#[cfg(not(windows))]
fn show_error_dialog(title: &str, text: &str) -> bool {
    eprintln!("{title}\n\n{text}");
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_gpui_graphics_panics() {
        assert_eq!(
            classify("failed to initialize DirectX: Creating DirectX devices: Getting DXGI adapter"),
            CrashKind::GraphicsUnavailable
        );
        assert_eq!(
            classify("Device lost: DirectXRenderer failed to recover from lost device after multiple attempts"),
            CrashKind::GraphicsLost
        );
        assert_eq!(classify("called `Option::unwrap()` on a `None` value"), CrashKind::Other);
    }

    #[test]
    fn graphics_dialogs_name_the_log_file() {
        let path = PathBuf::from(r"C:\Users\test\AppData\Local\dk.mahoje.tingskriver\logs\tingskriver-app.log");
        for kind in [CrashKind::GraphicsUnavailable, CrashKind::GraphicsLost] {
            let (title, text) = dialog_text(kind, Some(&path)).unwrap();
            assert!(title.starts_with("Tingskriver"));
            assert!(text.contains("tingskriver-app.log"), "{text}");
            assert!(text.contains("Prøv igen"));
        }
        let (_, text) = dialog_text(CrashKind::GraphicsLost, None).unwrap();
        assert!(text.contains("Logfilen kunne ikke oprettes"));
    }

    #[test]
    fn other_panics_get_no_dialog() {
        assert_eq!(dialog_text(CrashKind::Other, None), None);
    }
}
