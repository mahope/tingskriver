//! The console of the terminal a Windows release build was started from.
//!
//! The release build is a GUI program without a console of its own. Started
//! from a terminal it borrows that terminal's console while startup can still
//! have something to say (invalid arguments, `--diagnose`, a refused
//! hand-over) and lets go as soon as the process runs on as the application.
//! Held on to, the terminal would receive log lines for as long as the app
//! runs, and Ctrl+C or closing the terminal would kill the app and any
//! dictation in progress. Elsewhere, and in debug builds that have a console
//! of their own, both calls do nothing.

/// The standard handles that the console [`attach`] attached to put in place
/// and that are still in place: the ones [`release`] must clear once the
/// console is gone. `set_by_attach` holds (slot, handle) for each handle that
/// was not redirected before attaching and that attaching filled in; a handle
/// the process has replaced since is left alone. Redirected handles never
/// appear in `set_by_attach`.
#[cfg_attr(not(windows), allow(dead_code))]
fn handles_to_clear(set_by_attach: &[(u32, isize)], current: impl Fn(u32) -> isize) -> Vec<u32> {
    set_by_attach
        .iter()
        .filter(|&&(slot, handle)| current(slot) == handle)
        .map(|&(slot, _)| slot)
        .collect()
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::Mutex;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process: u32) -> i32;
        fn FreeConsole() -> i32;
        fn GetStdHandle(kind: u32) -> *mut c_void;
        fn SetStdHandle(kind: u32, handle: *mut c_void) -> i32;
    }

    /// `None` until this process has attached; then the standard handles the
    /// attach filled in (slot, handle), so that only a console it attached to
    /// is ever released and only handles it set are ever cleared.
    static ATTACHED: Mutex<Option<Vec<(u32, isize)>>> = Mutex::new(None);

    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE.
    const STD_HANDLES: [u32; 3] = [-10i32 as u32, -11i32 as u32, -12i32 as u32];

    fn is_set(handle: *mut c_void) -> bool {
        !handle.is_null() && handle as isize != -1
    }

    /// Attach to the console of the parent process, if it has one. Redirected
    /// standard handles are kept: stdout and stderr need not share a target.
    pub fn attach() {
        unsafe {
            let before = STD_HANDLES.map(|kind| (kind, GetStdHandle(kind)));
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                return;
            }
            let mut filled_in = Vec::new();
            for (kind, handle) in before {
                if is_set(handle) {
                    SetStdHandle(kind, handle);
                } else {
                    // Not redirected: the console's handle, if attaching
                    // put one in place.
                    let console = GetStdHandle(kind);
                    if is_set(console) {
                        filled_in.push((kind, console as isize));
                    }
                }
            }
            *ATTACHED.lock().unwrap_or_else(|e| e.into_inner()) = Some(filled_in);
        }
    }

    /// Let go of the console [`attach`] attached to. Redirected handles stay
    /// valid. The handles the console had supplied are cleared, because
    /// `FreeConsole` closes them but leaves their numbers in the process,
    /// where the next file the app opens could be given the same number and
    /// receive every log line. A cleared handle is the state of a program
    /// that never had a console: Rust's `print!`, `eprintln!` and the logger
    /// treat it as an invalid handle and write nothing, without panicking.
    pub fn release() {
        let Some(filled_in) = ATTACHED.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        unsafe {
            let ours = super::handles_to_clear(&filled_in, |kind| GetStdHandle(kind) as isize);
            // Clear first, so no write can reach a handle number that
            // `FreeConsole` has just closed and a new file could reuse.
            for kind in ours {
                SetStdHandle(kind, std::ptr::null_mut());
            }
            FreeConsole();
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn attach() {}
    pub fn release() {}
}

pub use imp::{attach, release};

#[cfg(test)]
mod tests {
    use super::handles_to_clear;

    #[test]
    fn only_handles_the_attach_set_and_still_in_place_are_cleared() {
        let set_by_attach = [(10, 100), (11, 110), (12, 120)];
        // All still the console's: all cleared.
        assert_eq!(
            handles_to_clear(&set_by_attach, |slot| match slot {
                10 => 100,
                11 => 110,
                _ => 120,
            }),
            vec![10, 11, 12]
        );
        // The app has redirected stderr since: that one stays.
        assert_eq!(
            handles_to_clear(&set_by_attach, |slot| if slot == 12 {
                999
            } else {
                slot as isize * 10
            }),
            vec![10, 11]
        );
        // Nothing was filled in (everything redirected): nothing to clear,
        // whatever the current handles are.
        assert!(handles_to_clear(&[], |_| 5).is_empty());
    }
}
