//! What installing, updating and uninstalling do beyond copying files.
//!
//! The installer (Velopack, `installer/README.md`) starts the app with a
//! hook argument after install, after update and before uninstall; the app
//! calls [`after_install`], [`after_update`] or [`before_uninstall`] and
//! exits. Each must finish within Velopack's 15-30 s limit, so they only
//! touch the registry and a few files. Everything is per user: no
//! administrator, no machine-wide keys.

pub mod autostart;
pub mod legacy;
pub mod user_data;
#[cfg(windows)]
mod win;

use std::path::Path;

/// After the first install: start with Windows and tidy up after earlier
/// builds. `exe` is the installed app.
pub fn after_install(exe: &Path) -> Vec<String> {
    let mut report = Vec::new();
    match autostart::enable(exe) {
        Ok(()) => report.push(format!("start with Windows: {}", autostart::run_command(exe))),
        Err(e) => report.push(format!("could not enable start with Windows: {e}")),
    }
    report.extend(remove_legacy_leftovers());
    report
}

/// After an update. The install folder does not move between versions, so
/// the start-with-Windows entry stays valid; it is only rewritten when it
/// exists and points elsewhere, so a user who turned it off keeps it off.
pub fn after_update(exe: &Path) -> Vec<String> {
    let mut report = Vec::new();
    let wanted = autostart::run_command(exe);
    if let Some(current) = autostart::current() {
        if current != wanted {
            match autostart::enable(exe) {
                Ok(()) => report.push(format!("start with Windows moved to {wanted}")),
                Err(e) => report.push(format!("could not update start with Windows: {e}")),
            }
        }
    }
    report.extend(remove_legacy_leftovers());
    report
}

/// Before uninstall: stop starting with Windows. The user's data and keys
/// stay unless they asked for them to go ([`user_data`]).
pub fn before_uninstall() -> Vec<String> {
    let mut report = Vec::new();
    if let Err(e) = autostart::disable() {
        report.push(format!("could not remove start with Windows: {e}"));
    }
    report.extend(user_data::remove_if_requested());
    report
}

/// Leftover `meetily.exe`, shortcuts and registry entries ([`legacy`]).
pub fn remove_legacy_leftovers() -> Vec<String> {
    let exists = |path: &str| Path::new(path).exists();
    let mut report = legacy::remove_stale_files(&legacy::LegacyLocations::for_current_user(), exists);
    report.extend(remove_legacy_registry(exists));
    report
}

#[cfg(windows)]
fn remove_legacy_registry(exists: impl Fn(&str) -> bool + Copy) -> Vec<String> {
    let mut report = Vec::new();
    let values = win::string_values(autostart::RUN_KEY);
    for name in legacy::stale_run_values(&values, exists) {
        match win::delete_value(autostart::RUN_KEY, &name) {
            Ok(()) => report.push(format!("removed Run value {name}")),
            Err(e) => report.push(format!("could not remove Run value {name}: {e}")),
        }
    }
    for entry in win::uninstall_entries() {
        if legacy::uninstall_entry_is_stale(&entry, exists) {
            match win::delete_uninstall_entry(&entry.key) {
                Ok(()) => report.push(format!("removed uninstall entry {}", entry.key)),
                Err(e) => report.push(format!("could not remove uninstall entry {}: {e}", entry.key)),
            }
        }
    }
    report
}

#[cfg(not(windows))]
fn remove_legacy_registry(_exists: impl Fn(&str) -> bool + Copy) -> Vec<String> {
    Vec::new()
}
