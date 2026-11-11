//! Obtain the account SID from the process token, never from environment variables.
use std::{ffi::c_void, io};

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    fn GetTokenInformation(
        token: *mut c_void,
        class: u32,
        data: *mut c_void,
        size: u32,
        needed: *mut u32,
    ) -> i32;
    fn ConvertSidToStringSidW(sid: *const c_void, text: *mut *mut u16) -> i32;
    fn GetSecurityInfo(
        handle: *mut c_void,
        object_type: u32,
        info: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut c_void,
        sacl: *mut *mut c_void,
        descriptor: *mut *mut c_void,
    ) -> u32;
    fn GetNamedSecurityInfoW(
        name: *const u16,
        object_type: u32,
        info: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut c_void,
        sacl: *mut *mut c_void,
        descriptor: *mut *mut c_void,
    ) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetNamedPipeServerProcessId(pipe: *mut c_void, pid: *mut u32) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

pub fn sid() -> io::Result<String> {
    unsafe { process_sid(GetCurrentProcess()) }
}

unsafe fn process_sid(process: *mut c_void) -> io::Result<String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(process, 8, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut needed = 0;
        GetTokenInformation(token, 1, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 || needed > 65536 {
            CloseHandle(token);
            return Err(io::Error::other("invalid token size"));
        }
        let mut data = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        let ok = GetTokenInformation(token, 1, data.as_mut_ptr().cast(), needed, &mut needed);
        let error = io::Error::last_os_error();
        CloseHandle(token);
        if ok == 0 {
            return Err(error);
        }
        sid_text(data[0] as *const c_void)
    }
}

unsafe fn sid_text(sid: *const c_void) -> io::Result<String> {
    unsafe {
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(sid)
    }
}

// OWNER_SECURITY_INFORMATION.
const OWNER: u32 = 1;

/// The owner SID of a security descriptor that `GetSecurityInfo` or
/// `GetNamedSecurityInfoW` returned, as text. Frees the descriptor.
unsafe fn take_owner(
    status: u32,
    owner: *mut c_void,
    descriptor: *mut c_void,
) -> io::Result<String> {
    unsafe {
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        // The owner SID lives inside the descriptor, which is freed last.
        let text = if owner.is_null() {
            Err(io::Error::other("pipe has no owner"))
        } else {
            sid_text(owner)
        };
        LocalFree(descriptor);
        text
    }
}

/// The account that owns the pipe object, as a SID string.
fn pipe_owner(pipe: &impl std::os::windows::io::AsRawHandle) -> io::Result<String> {
    // SE_KERNEL_OBJECT.
    const KERNEL_OBJECT: u32 = 6;
    unsafe {
        let mut owner = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        let status = GetSecurityInfo(
            pipe.as_raw_handle(),
            KERNEL_OBJECT,
            OWNER,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        );
        take_owner(status, owner, descriptor)
    }
}

/// The account that owns the pipe named `name`, read without opening it for
/// reading or writing (READ_CONTROL is enough), so it also works on a pipe
/// whose integrity label keeps a normal process from connecting.
fn named_pipe_owner(name: &str) -> io::Result<String> {
    // SE_FILE_OBJECT: pipes live in the file system namespace.
    const FILE_OBJECT: u32 = 1;
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    unsafe {
        let mut owner = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        let status = GetNamedSecurityInfoW(
            name.as_ptr(),
            FILE_OBJECT,
            OWNER,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        );
        take_owner(status, owner, descriptor)
    }
}

/// Opening the pipe `name` was refused with ACCESS_DENIED: see
/// [`super::open_denied_failure`]. Only a readable owner can name this
/// account's own elevated instance; everything else is `Other`.
pub fn open_denied(name: &str) -> super::Failure {
    let owner = named_pipe_owner(name).ok();
    let own = sid().ok();
    super::open_denied_failure(owner.as_deref(), own.as_deref())
}

const ERROR_ACCESS_DENIED: i32 = 5;

/// Check that the account running the pipe server is this account.
/// `ForeignOwner` is another account; `ElevatedOwner` is this account's own
/// instance running as administrator, whose token a normal process cannot
/// read; anything that cannot be told is `Other`.
pub fn verify_server(pipe: &impl std::os::windows::io::AsRawHandle) -> Result<(), super::Failure> {
    use super::Failure;
    unsafe {
        let mut pid = 0;
        if GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) == 0 {
            return Err(Failure::Other);
        }
        // PROCESS_QUERY_LIMITED_INFORMATION, TOKEN_QUERY only.
        let process = OpenProcess(0x1000, 0, pid);
        let server_sid = if process.is_null() {
            Err(io::Error::last_os_error())
        } else {
            let server_sid = process_sid(process);
            CloseHandle(process);
            server_sid
        };
        let own_sid = sid().map_err(|_| Failure::Other)?;
        match server_sid {
            Ok(server_sid) if server_sid == own_sid => Ok(()),
            Ok(_) => Err(Failure::ForeignOwner),
            // Both an elevated instance of this account and another
            // account's process refuse the query; the owner of the pipe
            // object tells them apart.
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
                Err(match pipe_owner(pipe) {
                    Ok(owner) => super::owner_failure(&owner, &own_sid),
                    Err(_) => Failure::Other,
                })
            }
            Err(_) => Err(Failure::Other),
        }
    }
}

#[repr(C)]
struct SecurityAttributes {
    length: u32,
    descriptor: *mut c_void,
    inherit: i32,
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
}

pub fn server(
    name: &str,
    first: bool,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    // Protected DACL: only the token's account may connect. No inherited
    // Everyone ACE. The Medium integrity label (no write up) lets a normal
    // process of this account connect to the pipe of an instance running as
    // administrator, which may otherwise carry that process's High label and
    // refuse it at open; a normal launch can then see that the instance is
    // elevated and exit with code 5 instead of being turned away. Low
    // integrity (sandboxed) processes of the same account stay out, which
    // `LW` would let in.
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})S:(ML;;NW;;;ME)", sid()?)
        .encode_utf16()
        .chain([0])
        .collect();
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = SecurityAttributes {
            length: std::mem::size_of::<SecurityAttributes>() as u32,
            descriptor,
            inherit: 0,
        };
        let result = tokio::net::windows::named_pipe::ServerOptions::new()
            .reject_remote_clients(true)
            .first_pipe_instance(first)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SecurityAttributes).cast(),
            );
        LocalFree(descriptor);
        result
    }
}
