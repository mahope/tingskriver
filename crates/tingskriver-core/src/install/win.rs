//! Registry and Credential Manager calls for the install hooks. Everything
//! is under `HKEY_CURRENT_USER`; nothing needs administrator.

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredEnumerateW, CredFree, CREDENTIALW, CRED_ENUMERATE_FLAGS, CRED_TYPE,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW,
    RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE,
    KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
};

use super::legacy::UninstallEntry;

/// Uninstall entries of per-user programs.
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(subkey: &str, access: REG_SAM_FLAGS) -> Result<Option<Key>, String> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(subkey),
            None,
            access,
            &mut key,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(Some(Key(key))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        error => Err(format!("RegOpenKeyExW({subkey}): {error:?}")),
    }
}

/// Open a key for writing, creating it and any missing parents first. A new
/// user profile has no `...\CurrentVersion\Run` key until something writes
/// one, so opening it alone made "start with Windows" fail on first install.
fn create(subkey: &str, access: REG_SAM_FLAGS) -> Result<Key, String> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(subkey),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            access,
            None,
            &mut key,
            None,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(Key(key))
    } else {
        Err(format!("RegCreateKeyExW({subkey}): {status:?}"))
    }
}

fn wide_to_string(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// Does the key exist?
pub fn key_exists(subkey: &str) -> bool {
    matches!(open(subkey, KEY_READ), Ok(Some(_)))
}

/// A string value (`REG_SZ` or `REG_EXPAND_SZ`, not expanded).
pub fn get_string(subkey: &str, name: &str) -> Option<String> {
    let (subkey, name) = (HSTRING::from(subkey), HSTRING::from(name));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut size = 0u32;
    unsafe {
        if RegGetValueW(
            HKEY_CURRENT_USER,
            &subkey,
            &name,
            flags,
            None,
            None,
            Some(&mut size),
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut size = (buffer.len() * 2) as u32;
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            &subkey,
            &name,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        );
        (status == ERROR_SUCCESS).then(|| wide_to_string(&buffer))
    }
}

pub fn set_string(subkey: &str, name: &str, value: &str) -> Result<(), String> {
    let key = create(subkey, KEY_SET_VALUE)?;
    let data: Vec<u8> = value
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    let status = unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, REG_SZ, Some(&data)) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("RegSetValueExW({name}): {status:?}"))
    }
}

/// Delete a value; not an error when it is not there.
pub fn delete_value(subkey: &str, name: &str) -> Result<(), String> {
    let Some(key) = open(subkey, KEY_SET_VALUE)? else {
        return Ok(());
    };
    match unsafe { RegDeleteValueW(key.0, &HSTRING::from(name)) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        error => Err(format!("RegDeleteValueW({name}): {error:?}")),
    }
}

/// Names and string data of the values of a key.
pub fn string_values(subkey: &str) -> Vec<(String, String)> {
    let Ok(Some(key)) = open(subkey, KEY_READ) else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for index in 0.. {
        let mut name = vec![0u16; 16_384];
        let mut name_len = name.len() as u32;
        let mut data = vec![0u8; 64 * 1024];
        let mut data_len = data.len() as u32;
        let mut kind = 0u32;
        let status = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status != ERROR_SUCCESS {
            continue;
        }
        if kind == REG_SZ.0 || kind == windows::Win32::System::Registry::REG_EXPAND_SZ.0 {
            let wide: Vec<u16> = data[..data_len as usize]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            values.push((
                wide_to_string(&name[..name_len as usize]),
                wide_to_string(&wide),
            ));
        }
    }
    values
}

/// The per-user uninstall entries.
pub fn uninstall_entries() -> Vec<UninstallEntry> {
    let Ok(Some(key)) = open(UNINSTALL_KEY, KEY_READ) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for index in 0.. {
        let mut name = vec![0u16; 256];
        let mut name_len = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status == ERROR_SUCCESS {
            names.push(wide_to_string(&name[..name_len as usize]));
        }
    }
    names
        .into_iter()
        .map(|name| {
            let subkey = format!(r"{UNINSTALL_KEY}\{name}");
            UninstallEntry {
                display_name: get_string(&subkey, "DisplayName").unwrap_or_default(),
                display_icon: get_string(&subkey, "DisplayIcon").unwrap_or_default(),
                uninstall_string: get_string(&subkey, "UninstallString").unwrap_or_default(),
                key: name,
            }
        })
        .collect()
}

pub fn delete_uninstall_entry(name: &str) -> Result<(), String> {
    let Some(key) = open(UNINSTALL_KEY, KEY_WRITE | KEY_READ)? else {
        return Ok(());
    };
    match unsafe { RegDeleteTreeW(key.0, &HSTRING::from(name)) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        error => Err(format!("RegDeleteTreeW({name}): {error:?}")),
    }
}

/// Target and type of every credential whose target starts with `prefix`.
fn enumerate_credentials(prefix: &str) -> Result<Vec<(String, CRED_TYPE)>, String> {
    let filter = HSTRING::from(format!("{prefix}*"));
    let mut count = 0u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    if let Err(e) = unsafe {
        CredEnumerateW(
            &filter,
            Some(CRED_ENUMERATE_FLAGS(0)),
            &mut count,
            &mut list,
        )
    } {
        // ERROR_NOT_FOUND: no credentials match.
        if e.code() == windows::Win32::Foundation::ERROR_NOT_FOUND.to_hresult() {
            return Ok(Vec::new());
        }
        return Err(format!("CredEnumerateW: {e}"));
    }
    let mut targets = Vec::new();
    unsafe {
        for credential in std::slice::from_raw_parts(list, count as usize) {
            let credential = &**credential;
            if !credential.TargetName.is_null() {
                targets.push((
                    credential.TargetName.to_string().unwrap_or_default(),
                    credential.Type,
                ));
            }
        }
        CredFree(list as *const std::ffi::c_void);
    }
    // The filter is a wildcard match; keep only exact prefixes.
    targets.retain(|(target, _)| target.starts_with(prefix));
    Ok(targets)
}

/// The target of every credential that starts with `prefix`.
pub fn list_credentials_with_prefix(prefix: &str) -> Result<Vec<String>, String> {
    Ok(enumerate_credentials(prefix)?
        .into_iter()
        .map(|(target, _)| target)
        .collect())
}

/// Delete every generic credential whose target starts with `prefix`.
pub fn delete_credentials_with_prefix(prefix: &str) -> Result<usize, String> {
    let mut removed = 0;
    let mut failed = 0;
    for (target, kind) in enumerate_credentials(prefix)? {
        match unsafe { CredDeleteW(&HSTRING::from(target.as_str()), kind, None) } {
            Ok(()) => removed += 1,
            // Already gone: that is what we wanted.
            Err(e) if e.code() == windows::Win32::Foundation::ERROR_NOT_FOUND.to_hresult() => {}
            Err(_) => failed += 1,
        }
    }
    if failed > 0 {
        // Never report success while entries remain.
        return Err(format!("{failed} credential entries could not be deleted"));
    }
    Ok(removed)
}

/// Remove only our per-user protocol tree; missing keys are harmless.
pub fn delete_protocol() -> Result<(), String> {
    match unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(super::protocol::KEY)) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        error => Err(format!("RegDeleteTreeW(protocol): {error:?}")),
    }
}
