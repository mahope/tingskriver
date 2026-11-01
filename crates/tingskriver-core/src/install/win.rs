//! Registry and Credential Manager calls for the install hooks. Everything
//! is under `HKEY_CURRENT_USER`; nothing needs administrator.

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::Security::Credentials::{CredDeleteW, CredEnumerateW, CredFree, CREDENTIALW, CRED_ENUMERATE_FLAGS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, KEY_WRITE, REG_SAM_FLAGS, REG_SZ,
    RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
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
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(subkey), None, access, &mut key) };
    match status {
        ERROR_SUCCESS => Ok(Some(Key(key))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        error => Err(format!("RegOpenKeyExW({subkey}): {error:?}")),
    }
}

fn wide_to_string(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// A string value (`REG_SZ` or `REG_EXPAND_SZ`, not expanded).
pub fn get_string(subkey: &str, name: &str) -> Option<String> {
    let (subkey, name) = (HSTRING::from(subkey), HSTRING::from(name));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut size = 0u32;
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, &subkey, &name, flags, None, None, Some(&mut size)) != ERROR_SUCCESS {
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
    let Some(key) = open(subkey, KEY_SET_VALUE)? else {
        return Err(format!("{subkey} does not exist"));
    };
    let data: Vec<u8> = value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
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
            values.push((wide_to_string(&name[..name_len as usize]), wide_to_string(&wide)));
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
        let status =
            unsafe { RegEnumKeyExW(key.0, index, Some(PWSTR(name.as_mut_ptr())), &mut name_len, None, None, None, None) };
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

/// Delete every generic credential whose target starts with `prefix`.
pub fn delete_credentials_with_prefix(prefix: &str) -> Result<usize, String> {
    let filter = HSTRING::from(format!("{prefix}*"));
    let mut count = 0u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    if let Err(e) = unsafe { CredEnumerateW(&filter, Some(CRED_ENUMERATE_FLAGS(0)), &mut count, &mut list) } {
        // ERROR_NOT_FOUND: no credentials match.
        if e.code() == windows::Win32::Foundation::ERROR_NOT_FOUND.to_hresult() {
            return Ok(0);
        }
        return Err(format!("CredEnumerateW: {e}"));
    }
    let mut targets = Vec::new();
    unsafe {
        for credential in std::slice::from_raw_parts(list, count as usize) {
            let credential = &**credential;
            if !credential.TargetName.is_null() {
                targets.push((credential.TargetName.to_string().unwrap_or_default(), credential.Type));
            }
        }
        CredFree(list as *const std::ffi::c_void);
    }
    let mut removed = 0;
    for (target, kind) in targets {
        if target.starts_with(prefix) && unsafe { CredDeleteW(&HSTRING::from(target.as_str()), kind, None) }.is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}
