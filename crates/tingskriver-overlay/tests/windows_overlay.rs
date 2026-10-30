//! The real window on Windows: show latency, no frames while hidden, no
//! focus theft and the window styles from the issue.
#![cfg(windows)]

use std::time::{Duration, Instant};

use tingskriver_overlay::theme::timing;
use tingskriver_overlay::windows::{Overlay, Stats, SHOW_BUDGET};
use tingskriver_overlay::PillState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, IsWindowVisible, GWL_EXSTYLE, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

fn wait_for(overlay: &Overlay, done: impl Fn(&Stats) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if done(&overlay.stats()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    false
}

fn visible(overlay: &Overlay) -> bool {
    unsafe { IsWindowVisible(overlay.hwnd() as _) != 0 }
}

fn listening() -> PillState {
    PillState::Listening {
        level: 0.4,
        hands_free: false,
    }
}

#[test]
fn window_has_the_no_focus_styles() {
    let overlay = Overlay::new().expect("overlay window");
    let style = unsafe { GetWindowLongPtrW(overlay.hwnd() as _, GWL_EXSTYLE) } as u32;
    for (flag, name) in [
        (WS_EX_LAYERED, "WS_EX_LAYERED"),
        (WS_EX_TRANSPARENT, "WS_EX_TRANSPARENT"),
        (WS_EX_TOPMOST, "WS_EX_TOPMOST"),
        (WS_EX_NOACTIVATE, "WS_EX_NOACTIVATE"),
        (WS_EX_TOOLWINDOW, "WS_EX_TOOLWINDOW"),
    ] {
        assert!(style & flag != 0, "{name} missing");
    }
    assert!(!visible(&overlay), "created hidden");
}

#[test]
fn shows_fast_draws_only_while_visible_and_keeps_focus() {
    let overlay = Overlay::new().expect("overlay window");
    let foreground = unsafe { GetForegroundWindow() };
    let mut latencies = Vec::new();

    for _ in 0..5 {
        let shows = overlay.stats().shows;
        overlay.set_state(listening());
        assert!(wait_for(&overlay, |s| s.shows > shows), "pill never shown");
        latencies.push(overlay.stats().last_show_latency.expect("latency measured"));
        assert!(visible(&overlay));
        assert_eq!(
            unsafe { GetForegroundWindow() },
            foreground,
            "the pill took focus"
        );

        // About 30 fps while visible.
        let frames = overlay.stats().frames;
        std::thread::sleep(Duration::from_millis(300));
        let drawn = overlay.stats().frames - frames;
        assert!((4..=20).contains(&drawn), "{drawn} frames in 300 ms");

        overlay.set_state(PillState::Idle);
        std::thread::sleep(Duration::from_millis(timing::FADE_OUT_MS + 150));
        assert!(!visible(&overlay), "still visible after fade-out");
    }

    // Hidden: not a single frame.
    let frames = overlay.stats().frames;
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(overlay.stats().frames, frames, "drew while hidden");

    latencies.sort();
    eprintln!("show latencies: {latencies:?}");
    let median = latencies[latencies.len() / 2];
    assert!(median < SHOW_BUDGET, "median show latency {median:?}");
}

#[test]
fn finished_states_hide_by_themselves() {
    let overlay = Overlay::new().expect("overlay window");
    overlay.set_state(PillState::Done);
    assert!(wait_for(&overlay, |s| s.shows == 1));
    std::thread::sleep(Duration::from_millis(
        timing::DONE_HIDE_MS + timing::FADE_OUT_MS + 300,
    ));
    assert!(!visible(&overlay));
}

#[test]
fn hide_and_show_keep_the_state() {
    let overlay = Overlay::new().expect("overlay window");
    overlay.set_state(PillState::Processing);
    assert!(wait_for(&overlay, |s| s.shows == 1));
    overlay.hide();
    std::thread::sleep(Duration::from_millis(timing::FADE_OUT_MS + 150));
    assert!(!visible(&overlay));
    overlay.show();
    assert!(wait_for(&overlay, |s| s.shows == 2));
    assert!(visible(&overlay));
}
