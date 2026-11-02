// Keep normal release startup free of a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;

fn main() {
    let args = args::StartupArgs::from_env();
    if args.diagnose {
        #[cfg(windows)]
        unsafe {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn AttachConsole(process: u32) -> i32;
                fn GetStdHandle(kind: u32) -> *mut std::ffi::c_void;
            }
            // Preserve redirected handles; otherwise attach to the invoking terminal.
            let stdout = GetStdHandle(-11i32 as u32);
            if stdout.is_null() || stdout as isize == -1 {
                AttachConsole(u32::MAX);
            }
        }
        let report = tingskriver_core::diagnose::Report::collect(
            &tingskriver_core::diagnose::system_probe(),
        );
        let output = if args.json {
            match report.to_json() {
                Ok(json) => json,
                Err(_) => {
                    eprintln!("Diagnosen kunne ikke eksporteres.");
                    std::process::exit(1);
                }
            }
        } else {
            report.to_text()
        };
        println!("{output}");
        std::process::exit(i32::from(report.has_errors()));
    }

    tingskriver_app::run();
}
