// Release builds have no console window on Windows; the log file and the
// error dialogs in `crash` tell what happened.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tingskriver_app::run();
}
