//! A panic in a background thread ends up in the log file with a backtrace
//! (#34). Its own test binary, because the logger and the panic hook are
//! global to the process.

#[test]
fn panic_in_background_thread_is_logged_with_backtrace() {
    let dir = tempfile::tempdir().unwrap();
    let log = tingskriver_core::logging::init_in(Some(dir.path()), "panic.log").expect("log file");
    assert_eq!(tingskriver_core::logging::log_file(), Some(log.clone()));

    let result = std::thread::Builder::new()
        .name("lydoptager".to_string())
        .spawn(|| panic!("simulated crash in the recording thread"))
        .unwrap()
        .join();
    assert!(result.is_err(), "the thread panicked");

    let logged = std::fs::read_to_string(&log).unwrap();
    let entry = logged
        .split("PANIC in thread")
        .nth(1)
        .unwrap_or_else(|| panic!("no panic in the log:\n{logged}"));
    assert!(entry.starts_with(" 'lydoptager' at "), "{entry}");
    assert!(entry.contains("panic_hook.rs:"), "location: {entry}");
    assert!(
        entry.contains("simulated crash in the recording thread"),
        "{entry}"
    );
    // force_capture: a real backtrace even without RUST_BACKTRACE.
    assert!(entry.contains("backtrace:"), "{entry}");
    assert!(
        entry.contains("panic_in_background_thread_is_logged_with_backtrace"),
        "backtrace frames: {entry}"
    );

    // The startup markers went to the same file.
    assert!(logged.contains("Startup: process start after"), "{logged}");
    tingskriver_core::logging::startup_phase("DB ready");
    log::logger().flush();
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(logged.contains("Startup: DB ready after"), "{logged}");
}
