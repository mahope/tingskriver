//! Per-user URL association. Values are created through win::set_string/create.
use std::path::Path;

pub const KEY: &str = r"Software\Classes\tingskriver";

pub fn command(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

#[cfg(windows)]
pub fn register(exe: &Path) -> Result<(), String> {
    super::win::set_string(KEY, "", "URL:Tingskriver")?;
    super::win::set_string(KEY, "URL Protocol", "")?;
    super::win::set_string(&format!(r"{KEY}\shell\open\command"), "", &command(exe))
}

#[cfg(windows)]
pub fn unregister() -> Result<(), String> {
    super::win::delete_protocol()
}

#[cfg(not(windows))]
pub fn register(_exe: &Path) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn unregister() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_executable_and_url() {
        assert_eq!(
            command(Path::new(r"C:\Program Files\Tingskriver\app.exe")),
            r#""C:\Program Files\Tingskriver\app.exe" "%1""#
        );
        assert_eq!(KEY, r"Software\Classes\tingskriver");
    }
}
