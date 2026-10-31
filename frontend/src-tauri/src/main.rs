#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    tingskriver_core::logging::mark_process_start();
    std::env::set_var("RUST_LOG", "info");
    // Log file and panic hook: release builds have no console.
    app_lib::file_logger::init();
    app_lib::run();
}
