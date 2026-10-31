//! The keyboard hook as the host runs it, on a real Windows desktop.
//!
//! Starts the host like the app does (dictation service, hook thread, tray),
//! sends 10,000 key presses (F24, which no program uses) and checks the
//! hook's own timing summary in the log: p99 under 1 ms, and no key name or
//! code anywhere in the log. It types into the foreground window, so it only
//! runs on request (do not touch the keyboard meanwhile):
//!
//! ```powershell
//! cargo test -p tingskriver-host --test hook_through_host -- --ignored --nocapture
//! ```
#![cfg(windows)]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_F24,
};

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let line = format!("{} {}: {}", record.level(), record.target(), record.args());
        println!("{line}");
        LINES.lock().unwrap().push(line);
    }

    fn flush(&self) {}
}

fn key(flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_F24,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[test]
#[ignore = "types 10,000 keys on the desktop; run on request"]
fn ten_thousand_presses_through_the_host() {
    log::set_logger(&Capture).unwrap();
    log::set_max_level(log::LevelFilter::Debug);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    tingskriver_core::runtime::set_handle(runtime.handle().clone());
    let dir = std::env::temp_dir().join(format!("tingskriver-hook-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (host, _requests) = tingskriver_host::Host::start(dir.clone(), runtime.handle().clone());

    // Wait for the dictation service to install the hook.
    let started = Instant::now();
    loop {
        if tingskriver_core::dictation::service::get().is_ok_and(|s| s.shortcut_active()) {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the hook did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // 10,000 presses = 20,000 hook calls (down and up): two summaries.
    for _ in 0..10_000 {
        let inputs = [key(0), key(KEYEVENTF_KEYUP)];
        let sent = unsafe { SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(sent, 2, "SendInput was blocked (UIPI or no desktop)");
    }
    // Let the hook thread log the last summary.
    std::thread::sleep(Duration::from_millis(500));
    drop(host);
    drop(runtime);

    let lines = LINES.lock().unwrap().clone();
    let summaries: Vec<&String> = lines
        .iter()
        .filter(|l| l.contains("Keyboard hook timing"))
        .collect();
    assert!(
        summaries.len() >= 2,
        "expected two timing summaries, got {summaries:?}"
    );
    for summary in &summaries[..2] {
        let p99: u64 = summary
            .split("p99 ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("no p99 in {summary}"));
        // The summary is logged after the callback, so a few more calls may count.
        let calls: u64 = summary
            .split(" calls")
            .next()
            .and_then(|head| head.rsplit(' ').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("no call count in {summary}"));
        assert!(calls >= 10_000, "{summary}");
        assert!(p99 < 1_000, "hook p99 {p99} us is over 1 ms: {summary}");
    }
    // Nothing about which key was pressed reaches the log.
    for line in &lines {
        let lower = line.to_lowercase();
        for needle in ["f24", "0x87", "vk_", "vkcode", "virtual-key", "scancode"] {
            assert!(
                !lower.contains(needle),
                "log line mentions a key ({needle}): {line}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
