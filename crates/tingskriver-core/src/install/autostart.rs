//! Start with Windows (D4): a value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` that starts the app
//! with `--hidden`. Per user, no administrator needed.

use std::path::Path;

/// Registry key of the per-user Run entries.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Name of our value under [`RUN_KEY`].
pub const RUN_VALUE_NAME: &str = "Tingskriver";

/// The argument that starts the app without a window.
pub const HIDDEN_ARG: &str = "--hidden";

/// The command Windows runs at sign-in.
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\" {HIDDEN_ARG}", exe.display())
}

/// Start `exe` hidden when the user signs in.
#[cfg(windows)]
pub fn enable(exe: &Path) -> Result<(), String> {
    super::win::set_string(RUN_KEY, RUN_VALUE_NAME, &run_command(exe))
}

/// Stop starting with Windows. Not an error when it was off.
#[cfg(windows)]
pub fn disable() -> Result<(), String> {
    super::win::delete_value(RUN_KEY, RUN_VALUE_NAME)
}

/// The command registered now, if any.
#[cfg(windows)]
pub fn current() -> Option<String> {
    super::win::get_string(RUN_KEY, RUN_VALUE_NAME)
}

#[cfg(not(windows))]
pub fn enable(_exe: &Path) -> Result<(), String> {
    Err("Start med Windows findes kun på Windows".to_string())
}

#[cfg(not(windows))]
pub fn disable() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn current() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::legacy::exe_from_command;

    #[test]
    fn the_command_quotes_the_exe_and_starts_hidden() {
        let exe = Path::new(r"C:\Users\Anne Sofie\AppData\Local\TingskriverApp\current\tingskriver-app.exe");
        let command = run_command(exe);
        assert_eq!(
            command,
            r#""C:\Users\Anne Sofie\AppData\Local\TingskriverApp\current\tingskriver-app.exe" --hidden"#
        );
        assert_eq!(exe_from_command(&command).as_deref(), exe.to_str());
        assert!(crate::install::legacy::stale_run_values(&[("x".into(), command)], |_| false).is_empty());
    }
}
