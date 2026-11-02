use std::sync::OnceLock;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

// Store the handle as an integer because HANDLE is not Sync. It is private,
// never inherited or closed in Rust; Windows closes it on process termination.
static JOB: OnceLock<Result<isize, String>> = OnceLock::new();

pub(super) fn assign(pid: u32) -> Result<(), String> {
    let job = JOB
        .get_or_init(|| unsafe {
            let handle = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Err(error) = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            ) {
                let _ = CloseHandle(handle);
                return Err(error.to_string());
            }
            Ok(handle.0 as isize)
        })
        .as_ref()
        .map_err(Clone::clone)?;
    unsafe {
        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)
            .map_err(|e| e.to_string())?;
        let result =
            AssignProcessToJobObject(HANDLE(*job as *mut _), process).map_err(|e| e.to_string());
        let _ = CloseHandle(process);
        result
    }
}
