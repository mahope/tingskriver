//! Leftovers from earlier binary names.
//!
//! Before 0.5.0-dev.1 the Tauri app was built as `meetily.exe`. The Tauri
//! installer updated it to `tingskriver.exe` in the same folder, but left
//! the old exe, shortcuts that point to it and registry entries behind.
//! This module finds those leftovers. It only removes things that point to
//! an exe that no longer exists, or a `meetily.exe` lying next to a
//! `tingskriver.exe`; a working upstream Meetily installation is left alone.

use std::path::{Path, PathBuf};

/// Exe names of earlier builds.
pub const LEGACY_EXE_NAMES: &[&str] = &["meetily.exe"];

/// The exe of the Tauri app since the rename.
pub const TAURI_EXE_NAME: &str = "tingskriver.exe";

/// Shortcuts are small; anything larger is not a shortcut we made.
const MAX_SHORTCUT_BYTES: u64 = 256 * 1024;

/// Where leftovers can be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacyLocations {
    /// Folders the Tauri installer installed into.
    pub install_dirs: Vec<PathBuf>,
    /// Folders with shortcuts (Start menu, its Startup folder, Desktop).
    pub shortcut_dirs: Vec<PathBuf>,
}

impl LegacyLocations {
    /// The folders for the current user on Windows. `local` is
    /// `%LOCALAPPDATA%`, `roaming` `%APPDATA%`.
    pub fn from_roots(local: Option<PathBuf>, roaming: Option<PathBuf>, desktop: Option<PathBuf>) -> Self {
        let mut install_dirs = Vec::new();
        if let Some(local) = local {
            install_dirs.push(local.join("Tingskriver"));
            install_dirs.push(local.join("Programs").join("Tingskriver"));
        }
        let mut shortcut_dirs = Vec::new();
        if let Some(roaming) = roaming {
            let programs = roaming.join("Microsoft").join("Windows").join("Start Menu").join("Programs");
            shortcut_dirs.push(programs.join("Startup"));
            shortcut_dirs.push(programs.join("Tingskriver"));
            shortcut_dirs.push(programs.join("meetily"));
            shortcut_dirs.push(programs);
        }
        if let Some(desktop) = desktop {
            shortcut_dirs.push(desktop);
        }
        LegacyLocations { install_dirs, shortcut_dirs }
    }

    pub fn for_current_user() -> Self {
        Self::from_roots(dirs::data_local_dir(), dirs::data_dir(), dirs::desktop_dir())
    }
}

/// Is `name` the file name of an earlier build (case-insensitive)?
pub fn is_legacy_exe_name(name: &str) -> bool {
    LEGACY_EXE_NAMES.iter().any(|legacy| legacy.eq_ignore_ascii_case(name))
}

/// The file name at the end of a Windows path (either separator).
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// The exe a command line starts: `"C:\a b\x.exe" --hidden`,
/// `C:\a b\x.exe --hidden` or `x.exe`.
pub fn exe_from_command(command: &str) -> Option<String> {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        let end = rest.find('"')?;
        let exe = &rest[..end];
        return (!exe.is_empty()).then(|| exe.to_string());
    }
    if let Some(index) = command.to_ascii_lowercase().find(".exe") {
        return Some(command[..index + 4].to_string());
    }
    command.split_whitespace().next().map(str::to_string)
}

/// The paths of legacy exes a shortcut file mentions. A `.lnk` keeps its
/// target both as ANSI and as UTF-16 text; both are searched.
pub fn shortcut_legacy_targets(bytes: &[u8]) -> Vec<String> {
    let ansi: String = bytes
        .iter()
        .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '\0' })
        .collect();
    let mut texts = vec![ansi];
    for offset in 0..2 {
        let text: String = bytes
            .get(offset..)
            .unwrap_or_default()
            .chunks_exact(2)
            .map(|pair| char::from_u32(u16::from_le_bytes([pair[0], pair[1]]) as u32).unwrap_or('\0'))
            .collect();
        texts.push(text);
    }

    let mut targets = Vec::new();
    for text in &texts {
        for token in text.split(|c: char| c.is_control() || "\"<>|*?".contains(c)) {
            let lower = token.to_lowercase();
            for legacy in LEGACY_EXE_NAMES {
                let Some(index) = lower.find(legacy) else { continue };
                let path = &token[..index + legacy.len()];
                let Some(colon) = path.rfind(":\\") else { continue };
                let Some(start) = colon.checked_sub(1) else { continue };
                if !path[start..].starts_with(|c: char| c.is_ascii_alphabetic()) {
                    continue;
                }
                let target = path[start..].to_string();
                if !targets.iter().any(|t: &String| t.eq_ignore_ascii_case(&target)) {
                    targets.push(target);
                }
            }
        }
    }
    targets
}

/// Should a shortcut with these bytes go? Only when it points to a legacy
/// exe that is gone.
pub fn shortcut_is_stale(bytes: &[u8], exists: impl Fn(&str) -> bool) -> bool {
    let targets = shortcut_legacy_targets(bytes);
    !targets.is_empty() && targets.iter().all(|target| !exists(target))
}

/// Names of `HKCU\...\Run` values that start a legacy exe that is gone.
pub fn stale_run_values(values: &[(String, String)], exists: impl Fn(&str) -> bool) -> Vec<String> {
    values
        .iter()
        .filter(|(_, command)| {
            exe_from_command(command).is_some_and(|exe| is_legacy_exe_name(file_name(&exe)) && !exists(&exe))
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// An entry under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UninstallEntry {
    pub key: String,
    pub display_name: String,
    pub display_icon: String,
    pub uninstall_string: String,
}

/// An uninstall entry for a legacy build whose uninstaller is gone: it only
/// clutters "Installerede apps" and fails when clicked.
pub fn uninstall_entry_is_stale(entry: &UninstallEntry, exists: impl Fn(&str) -> bool) -> bool {
    let icon_exe = exe_from_command(&entry.display_icon.replace(',', " ")).unwrap_or_default();
    let mentions_legacy = is_legacy_exe_name(file_name(&icon_exe))
        || entry.display_name.to_lowercase().contains("meetily");
    if !mentions_legacy {
        return false;
    }
    match exe_from_command(&entry.uninstall_string) {
        Some(uninstaller) => !exists(&uninstaller),
        None => true,
    }
}

/// A `meetily.exe` lying next to `tingskriver.exe` is left over from the
/// rename. An install folder with only `meetily.exe` is a working old
/// installation and is left alone.
pub fn stale_exes_in(install_dir: &Path) -> Vec<PathBuf> {
    if !install_dir.join(TAURI_EXE_NAME).is_file() {
        return Vec::new();
    }
    LEGACY_EXE_NAMES
        .iter()
        .map(|name| install_dir.join(name))
        .filter(|path| path.is_file())
        .collect()
}

/// Shortcut files in `dir` (not recursive) that point to a legacy exe
/// that is gone.
pub fn stale_shortcuts_in(dir: &Path, exists: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut stale = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_lnk = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
        let small = entry.metadata().is_ok_and(|m| m.is_file() && m.len() <= MAX_SHORTCUT_BYTES);
        if !is_lnk || !small {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            if shortcut_is_stale(&bytes, &exists) {
                stale.push(path);
            }
        }
    }
    stale
}

/// Remove leftover exes and shortcuts. Returns what was removed and what
/// failed, for the log.
pub fn remove_stale_files(locations: &LegacyLocations, exists: impl Fn(&str) -> bool) -> Vec<String> {
    let mut report = Vec::new();
    let mut remove = |path: &Path| match std::fs::remove_file(path) {
        Ok(()) => report.push(format!("removed {}", path.display())),
        Err(e) => report.push(format!("could not remove {}: {e}", path.display())),
    };
    for dir in &locations.install_dirs {
        for exe in stale_exes_in(dir) {
            remove(&exe);
        }
    }
    for dir in &locations.shortcut_dirs {
        for shortcut in stale_shortcuts_in(dir, &exists) {
            remove(&shortcut);
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    /// Bytes shaped like a .lnk: binary header, the ANSI target, and the
    /// UTF-16 target at an odd offset.
    fn fake_lnk(target: &str) -> Vec<u8> {
        let mut bytes = vec![0x4c, 0, 0, 0, 0x01, 0x14, 0x02, 0, 0xff, 0x7f, 0x03];
        // ANSI copy as Windows-1252 writes it: æøå become single high bytes.
        bytes.extend(target.chars().map(|c| if c.is_ascii() { c as u8 } else { 0xf8 }));
        bytes.push(0);
        bytes.push(7);
        bytes.extend(utf16(target));
        bytes.extend([0, 0, 0x10, 0x00]);
        bytes
    }

    const OLD: &str = r"C:\Users\Søren\AppData\Local\Tingskriver\meetily.exe";

    #[test]
    fn command_lines_give_their_exe() {
        assert_eq!(
            exe_from_command(r#""C:\Program Files\x\meetily.exe" --hidden"#).as_deref(),
            Some(r"C:\Program Files\x\meetily.exe")
        );
        assert_eq!(
            exe_from_command(r"C:\Program Files\x\Meetily.EXE --minimized").as_deref(),
            Some(r"C:\Program Files\x\Meetily.EXE")
        );
        assert_eq!(exe_from_command("notepad").as_deref(), Some("notepad"));
        assert_eq!(exe_from_command(r#"  "" "#), None);
        assert_eq!(exe_from_command(""), None);
    }

    #[test]
    fn shortcut_targets_are_found_in_utf16_and_ansi() {
        assert_eq!(shortcut_legacy_targets(&fake_lnk(OLD)), vec![OLD.to_string()]);
        let ascii = r"D:\apps\meetily\MEETILY.exe";
        assert_eq!(shortcut_legacy_targets(&fake_lnk(ascii)), vec![ascii.to_string()]);
    }

    #[test]
    fn shortcuts_to_other_programs_are_not_legacy() {
        let current = r"C:\Users\x\AppData\Local\TingskriverApp\current\tingskriver-app.exe";
        assert!(shortcut_legacy_targets(&fake_lnk(current)).is_empty());
        assert!(!shortcut_is_stale(&fake_lnk(current), |_| false));
        assert!(shortcut_legacy_targets(b"meetily.exe without a drive").is_empty());
    }

    #[test]
    fn a_shortcut_goes_only_when_its_target_is_gone() {
        let lnk = fake_lnk(OLD);
        assert!(shortcut_is_stale(&lnk, |_| false));
        assert!(!shortcut_is_stale(&lnk, |path| path == OLD));
    }

    #[test]
    fn run_values_for_a_missing_legacy_exe_are_stale() {
        let values = vec![
            ("meetily".to_string(), format!("\"{OLD}\" --minimized")),
            ("Tingskriver".to_string(), r#""C:\x\current\tingskriver-app.exe" --hidden"#.to_string()),
            ("Meetily upstream".to_string(), r"C:\Users\x\AppData\Local\meetily\meetily.exe".to_string()),
        ];
        let upstream = r"C:\Users\x\AppData\Local\meetily\meetily.exe";
        assert_eq!(stale_run_values(&values, |p| p == upstream), vec!["meetily".to_string()]);
    }

    #[test]
    fn orphaned_meetily_uninstall_entries_are_stale() {
        let entry = UninstallEntry {
            key: "meetily".to_string(),
            display_name: "meetily".to_string(),
            display_icon: r"C:\x\meetily\meetily.exe,0".to_string(),
            uninstall_string: r#""C:\x\meetily\uninstall.exe""#.to_string(),
        };
        assert!(uninstall_entry_is_stale(&entry, |_| false));
        assert!(!uninstall_entry_is_stale(&entry, |p| p == r"C:\x\meetily\uninstall.exe"));

        // The Tauri app since the rename: not legacy even when orphaned.
        let tauri = UninstallEntry {
            key: "Tingskriver".to_string(),
            display_name: "Tingskriver".to_string(),
            display_icon: r"C:\x\Tingskriver\tingskriver.exe".to_string(),
            uninstall_string: r#""C:\x\Tingskriver\uninstall.exe""#.to_string(),
        };
        assert!(!uninstall_entry_is_stale(&tauri, |_| false));

        // 0.5.0-dev.0: named Tingskriver, icon still meetily.exe.
        let dev0 = UninstallEntry {
            display_icon: OLD.to_string(),
            ..tauri
        };
        assert!(uninstall_entry_is_stale(&dev0, |_| false));
    }

    #[test]
    fn locations_cover_start_menu_startup_and_desktop() {
        let locations = LegacyLocations::from_roots(
            Some(PathBuf::from("L")),
            Some(PathBuf::from("R")),
            Some(PathBuf::from("D")),
        );
        assert!(locations.install_dirs.contains(&PathBuf::from("L").join("Tingskriver")));
        let programs = PathBuf::from("R").join("Microsoft").join("Windows").join("Start Menu").join("Programs");
        assert!(locations.shortcut_dirs.contains(&programs));
        assert!(locations.shortcut_dirs.contains(&programs.join("Startup")));
        assert!(locations.shortcut_dirs.contains(&PathBuf::from("D")));
        assert_eq!(LegacyLocations::from_roots(None, None, None), LegacyLocations::default());
    }

    #[test]
    fn leftover_exe_and_shortcut_are_removed_but_a_live_install_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let renamed = root.path().join("renamed");
        let old_only = root.path().join("old-only");
        let shortcuts = root.path().join("shortcuts");
        for dir in [&renamed, &old_only, &shortcuts] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(renamed.join("tingskriver.exe"), b"new").unwrap();
        std::fs::write(renamed.join("meetily.exe"), b"old").unwrap();
        std::fs::write(old_only.join("meetily.exe"), b"old").unwrap();
        std::fs::write(shortcuts.join("meetily.lnk"), fake_lnk(OLD)).unwrap();
        std::fs::write(shortcuts.join("Tingskriver.LNK"), fake_lnk(OLD)).unwrap();
        let other = fake_lnk(r"C:\x\TingskriverApp\current\tingskriver-app.exe");
        std::fs::write(shortcuts.join("Tingskriver ny.lnk"), other).unwrap();
        std::fs::write(shortcuts.join("notes.txt"), fake_lnk(OLD)).unwrap();

        let locations = LegacyLocations {
            install_dirs: vec![renamed.clone(), old_only.clone(), root.path().join("missing")],
            shortcut_dirs: vec![shortcuts.clone()],
        };
        let report = remove_stale_files(&locations, |_| false);

        assert_eq!(report.len(), 3, "{report:?}");
        assert!(!renamed.join("meetily.exe").exists());
        assert!(renamed.join("tingskriver.exe").exists());
        assert!(old_only.join("meetily.exe").exists());
        assert!(!shortcuts.join("meetily.lnk").exists());
        assert!(!shortcuts.join("Tingskriver.LNK").exists());
        assert!(shortcuts.join("Tingskriver ny.lnk").exists());
        assert!(shortcuts.join("notes.txt").exists());
    }
}
