//! Crash dumps from Windows Error Reporting for native crashes.
//!
//! The panic hook in [`crate::logging`] catches Rust panics, but a native
//! abort (an access violation in whisper.cpp or ONNX Runtime, a stack
//! overflow) ends the process without running any Rust code. For those,
//! Windows itself can write a minidump: a per-user `LocalDumps` key for the
//! exe under
//! `HKCU\Software\Microsoft\Windows\Windows Error Reporting\LocalDumps`
//! makes WER save a small dump (`DumpType` 1) in [`crate::paths::dump_dir`].
//! Nothing is uploaded; the dumps stay on the PC until the user sends one.
//!
//! On by default and set again at every start (the exe can move with an
//! update). Turning it off removes the key; the choice is a marker file next
//! to the logs, so it holds without a database.

use std::path::{Path, PathBuf};

/// The registry key that holds the per-exe `LocalDumps` settings.
pub const LOCAL_DUMPS_KEY: &str = r"Software\Microsoft\Windows\Windows Error Reporting\LocalDumps";

/// Minidump: threads, stacks and loaded modules, no heap. Small enough to
/// send, and holds no meeting audio or text.
pub const DUMP_TYPE_MINI: u32 = 1;

/// WER keeps at most this many dumps and drops the oldest.
pub const DUMP_COUNT: u32 = 5;

/// When this file exists in the app's local data folder, crash dumps are off.
pub const DISABLED_MARKER: &str = "crash-dumps-disabled";

/// The `LocalDumps` subkey for one exe, e.g. `...\LocalDumps\tingskriver.exe`.
pub fn exe_key(exe_name: &str) -> String {
    format!(r"{LOCAL_DUMPS_KEY}\{exe_name}")
}

/// The file name of the running exe (`tingskriver.exe`, `tingskriver-app.exe`).
pub fn current_exe_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

fn marker_path(local_dir: &Path) -> PathBuf {
    local_dir.join(DISABLED_MARKER)
}

/// Whether crash dumps are on, given the app's local data folder.
pub fn enabled_in(local_dir: &Path) -> bool {
    !marker_path(local_dir).exists()
}

/// Remember the choice in `local_dir` (marker file present means off).
pub fn remember_in(local_dir: &Path, enabled: bool) -> std::io::Result<()> {
    let marker = marker_path(local_dir);
    if enabled {
        match std::fs::remove_file(&marker) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        std::fs::create_dir_all(local_dir)?;
        std::fs::write(
            marker,
            b"Crash dumps are turned off in Tingskriver's settings.\n",
        )
    }
}

/// Whether crash dumps are on (the default).
pub fn enabled() -> bool {
    crate::paths::local_data_dir().map_or(true, |dir| enabled_in(&dir))
}

/// Turn crash dumps on or off and remember it.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let dir = crate::paths::local_data_dir().ok_or("Could not determine the local data folder")?;
    remember_in(&dir, enabled)
        .map_err(|e| format!("Could not save the crash dump setting: {e}"))?;
    apply()
}

/// Make the registry match the setting: the `LocalDumps` key for the
/// running exe when on, no key when off. Does nothing outside Windows.
pub fn apply() -> Result<(), String> {
    let exe = current_exe_name().ok_or("Could not determine the exe name")?;
    if enabled() {
        let dir = crate::paths::dump_dir().ok_or("Could not determine the dump folder")?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        platform::register(&exe, &dir)?;
        log::info!("Crash dumps for {} go to {}", exe, dir.display());
    } else {
        platform::unregister(&exe)?;
        log::info!("Crash dumps for {} are turned off", exe);
    }
    Ok(())
}

/// [`apply`] on its own thread, so startup does not wait for the registry.
pub fn apply_in_background() {
    let spawned = std::thread::Builder::new()
        .name("crash-dumps".to_string())
        .spawn(|| {
            if let Err(e) = apply() {
                log::warn!("Crash dump setup failed: {}", e);
            }
        });
    if let Err(e) = spawned {
        log::warn!("Could not start crash dump setup: {}", e);
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_VALUE_TYPE,
    };

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn check(what: &str, status: WIN32_ERROR) -> Result<(), String> {
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("{what} failed (Win32 error {})", status.0))
        }
    }

    fn set_value(key: HKEY, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) -> Result<(), String> {
        let name_w = wide(name.as_ref());
        // SAFETY: `key` is an open key with KEY_SET_VALUE; name and data
        // outlive the call.
        let status =
            unsafe { RegSetValueExW(key, PCWSTR(name_w.as_ptr()), None, kind, Some(data)) };
        check(&format!("Setting {name}"), status)
    }

    pub fn register(exe_name: &str, dump_dir: &Path) -> Result<(), String> {
        let subkey = wide(super::exe_key(exe_name).as_ref());
        let mut key = HKEY::default();
        // SAFETY: valid null-terminated subkey; `key` receives the handle.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        check("Creating the LocalDumps key", status)?;

        let folder: Vec<u8> = wide(dump_dir.as_os_str())
            .iter()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let result = set_value(key, "DumpFolder", REG_EXPAND_SZ, &folder)
            .and_then(|_| {
                set_value(
                    key,
                    "DumpType",
                    REG_DWORD,
                    &super::DUMP_TYPE_MINI.to_le_bytes(),
                )
            })
            .and_then(|_| {
                set_value(
                    key,
                    "DumpCount",
                    REG_DWORD,
                    &super::DUMP_COUNT.to_le_bytes(),
                )
            });
        // SAFETY: `key` was opened above and is closed once.
        let _ = unsafe { RegCloseKey(key) };
        result
    }

    pub fn unregister(exe_name: &str) -> Result<(), String> {
        let subkey = wide(super::exe_key(exe_name).as_ref());
        // SAFETY: valid null-terminated subkey under HKCU.
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check("Removing the LocalDumps key", status)
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    pub fn register(_exe_name: &str, _dump_dir: &Path) -> Result<(), String> {
        Ok(())
    }

    pub fn unregister(_exe_name: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_per_exe_under_local_dumps() {
        assert_eq!(
            exe_key("tingskriver.exe"),
            r"Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\tingskriver.exe"
        );
    }

    #[test]
    fn on_by_default_and_the_choice_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("dk.mahoje.tingskriver");
        assert!(enabled_in(&local));
        remember_in(&local, false).unwrap();
        assert!(!enabled_in(&local));
        remember_in(&local, false).unwrap();
        assert!(!enabled_in(&local));
        remember_in(&local, true).unwrap();
        assert!(enabled_in(&local));
        remember_in(&local, true).unwrap();
        assert!(enabled_in(&local));
    }

    #[test]
    fn minidumps_only() {
        assert_eq!(DUMP_TYPE_MINI, 1);
        assert!(DUMP_COUNT > 0);
    }
}
