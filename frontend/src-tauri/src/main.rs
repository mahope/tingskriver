#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    // A copy of this exe finishing the move to the GPUI app (move_to_gpui.rs).
    if app_lib::move_to_gpui::run_helper_if_requested() {
        return;
    }
    tingskriver_core::logging::mark_process_start();
    std::env::set_var("RUST_LOG", "info");
    // Log file and panic hook: release builds have no console.
    app_lib::file_logger::init();
    app_lib::run();
}
