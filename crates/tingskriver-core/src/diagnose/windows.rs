use super::{Check, NativeProbe, State, SystemProbe};
use std::ffi::c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetLastError() -> u32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn FreeLibrary(module: *mut c_void) -> i32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
    fn OpenClipboard(window: *mut c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn RegisterHotKey(window: *mut c_void, id: i32, modifiers: u32, key: u32) -> i32;
    fn UnregisterHotKey(window: *mut c_void, id: i32) -> i32;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    fn GetTokenInformation(
        token: *mut c_void,
        class: u32,
        data: *mut c_void,
        length: u32,
        needed: *mut u32,
    ) -> i32;
    fn GetSidSubAuthorityCount(sid: *const c_void) -> *mut u8;
    fn GetSidSubAuthority(sid: *const c_void, index: u32) -> *mut u32;
    fn RegGetValueW(
        key: *mut c_void,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut c_void,
        size: *mut u32,
    ) -> i32;
}
#[repr(C)]
struct Version {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    service: [u16; 128],
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(version: *mut Version) -> i32;
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn consent(root: isize, desktop: bool) -> State {
    let mut path = String::from("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone");
    if desktop {
        path.push_str("\\NonPackaged");
    }
    let path = wide(&path);
    let value = wide("Value");
    let mut data = [0u16; 32];
    let mut size = std::mem::size_of_val(&data) as u32;
    // RRF_RT_REG_SZ limits the result to a bounded string. No registry data is exported.
    let result = unsafe {
        RegGetValueW(
            root as *mut c_void,
            path.as_ptr(),
            value.as_ptr(),
            2,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if result != 0 {
        return State::Unknown;
    }
    match String::from_utf16_lossy(&data[..data.iter().position(|c| *c == 0).unwrap_or(data.len())])
        .as_str()
    {
        "Allow" => State::Ok,
        "Deny" => State::Missing,
        _ => State::Unknown,
    }
}
struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn integrity(process: *mut c_void) -> Option<u32> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, 8, &mut token) } == 0 {
        return None;
    }
    let token = Handle(token);
    let mut needed = 0;
    unsafe {
        GetTokenInformation(token.0, 25, std::ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 || needed > 65536 {
        return None;
    }
    // Pointer alignment is required for TOKEN_MANDATORY_LABEL and its SID.
    let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token.0, 25, buffer.as_mut_ptr().cast(), needed, &mut needed) }
        == 0
    {
        return None;
    }
    let sid = buffer[0] as *const c_void;
    if sid.is_null() {
        return None;
    }
    let count = unsafe { GetSidSubAuthorityCount(sid) };
    if count.is_null() || unsafe { *count } == 0 {
        return None;
    }
    let level = unsafe { GetSidSubAuthority(sid, u32::from(*count - 1)) };
    if level.is_null() {
        None
    } else {
        Some(unsafe { *level })
    }
}
fn elevated_window() -> State {
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return State::Unknown;
    }
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, &mut pid);
    }
    if pid == 0 {
        return State::Unknown;
    }
    let process = unsafe { OpenProcess(0x1000, 0, pid) };
    if process.is_null() {
        return State::Unknown;
    }
    let process = Handle(process);
    match (
        integrity(unsafe { GetCurrentProcess() }),
        integrity(process.0),
    ) {
        (Some(ours), Some(theirs)) if theirs > ours => State::Missing,
        (Some(_), Some(_)) => State::Ok,
        _ => State::Unknown,
    }
}
impl SystemProbe for NativeProbe {
    fn os_version(&self) -> Option<(u32, u32, u32)> {
        let mut version = Version {
            size: std::mem::size_of::<Version>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform: 0,
            service: [0; 128],
        };
        if unsafe { RtlGetVersion(&mut version) } == 0 {
            Some((version.major, version.minor, version.build))
        } else {
            None
        }
    }
    fn check(&self, check: Check) -> State {
        match check {
            Check::Avx2 => {
                #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                {
                    if std::is_x86_feature_detected!("avx2") {
                        State::Ok
                    } else {
                        State::Missing
                    }
                }
                #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
                {
                    State::NotApplicable
                }
            }
            Check::Vulkan => {
                // LOAD_LIBRARY_SEARCH_SYSTEM32 excludes application/current-directory DLLs.
                let module = unsafe {
                    LoadLibraryExW(wide("vulkan-1.dll").as_ptr(), std::ptr::null_mut(), 0x800)
                };
                if module.is_null() {
                    State::Missing
                } else {
                    unsafe {
                        FreeLibrary(module);
                    }
                    State::Ok
                }
            }
            Check::Microphone => {
                let user = 0x80000001u32 as i32 as isize;
                let machine = 0x80000002u32 as i32 as isize;
                super::microphone::microphone_permission([
                    consent(user, false),
                    consent(machine, false),
                    consent(user, true),
                    consent(machine, true),
                ])
            }
            Check::SystemAudio => {
                if crate::audio::devices::default_output_device().is_ok() {
                    State::Ok
                } else {
                    State::Unknown
                }
            }
            Check::Shortcut => {
                let Some((modifiers, key)) = self.shortcut else {
                    return State::Unknown;
                };
                // A fresh thread cannot reuse a registration ID owned by the UI thread.
                std::thread::Builder::new()
                    .spawn(move || {
                        if unsafe { RegisterHotKey(std::ptr::null_mut(), 0x3751, modifiers, key) }
                            != 0
                        {
                            if unsafe { UnregisterHotKey(std::ptr::null_mut(), 0x3751) } != 0 {
                                State::Ok
                            } else {
                                State::Unknown
                            }
                        } else if unsafe { GetLastError() } == 1409 {
                            State::Missing
                        } else {
                            State::Unknown
                        }
                    })
                    .ok()
                    .and_then(|thread| thread.join().ok())
                    .unwrap_or(State::Unknown)
            }
            Check::Clipboard => {
                for attempt in 0..3 {
                    if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                        unsafe {
                            CloseClipboard();
                        }
                        return State::Ok;
                    }
                    if attempt < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
                State::Missing
            }
            Check::ElevatedWindow => elevated_window(),
        }
    }
}
