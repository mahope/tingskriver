//! API keys for dictation live in Windows Credential Manager, never in the
//! database, settings files or logs.

pub const SYV_TARGET: &str = "Tingskriver/syv.ai";

#[cfg(windows)]
pub fn read(target: &str) -> Result<Option<String>, String> {
    super::win::credential_read(target)
}

#[cfg(windows)]
pub fn write(target: &str, secret: &str) -> Result<(), String> {
    super::win::credential_write(target, secret)
}

#[cfg(windows)]
pub fn delete(target: &str) -> Result<(), String> {
    super::win::credential_delete(target)
}

#[cfg(not(windows))]
pub fn read(_target: &str) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(windows))]
pub fn write(_target: &str, _secret: &str) -> Result<(), String> {
    Err("Nøgler kan kun gemmes på Windows (Windows Legitimationsstyring)".to_string())
}

#[cfg(not(windows))]
pub fn delete(_target: &str) -> Result<(), String> {
    Ok(())
}

pub fn syv_key() -> Option<String> {
    match read(SYV_TARGET) {
        Ok(key) => key.filter(|k| !k.trim().is_empty()),
        Err(e) => {
            log::warn!("Could not read Syv.ai key from Credential Manager: {e}");
            None
        }
    }
}
