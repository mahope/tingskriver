#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    if tingskriver_core::stt::run_worker().is_err() {
        std::process::exit(1);
    }
}
