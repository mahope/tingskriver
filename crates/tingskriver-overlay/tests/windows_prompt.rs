//! The real prompt window on Windows: no focus theft and the window styles.
//! Whether a prompt is shown depends on the machine's notification state
//! (Focus Assist, full screen), so the test accepts "held back" when
//! Windows says the user must not be disturbed.
#![cfg(windows)]

use std::time::{Duration, Instant};

use tingskriver_overlay::prompt::{PromptOutcome, PromptRequest};
use tingskriver_overlay::windows_prompt::{query_notification_state, PromptEvent, PromptWindow};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, GetWindowLongPtrW, IsWindowVisible, GWL_EXSTYLE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

fn find_prompt() -> Option<isize> {
    let class: Vec<u16> = "TingskriverPrompt\0".encode_utf16().collect();
    // SAFETY: NUL-terminated class name.
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    (!hwnd.is_null()).then_some(hwnd as isize)
}

fn wait_for_prompt() -> Option<isize> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Some(hwnd) = find_prompt() {
            return Some(hwnd);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn prompt_is_shown_without_taking_focus_or_held_back_on_purpose() {
    let prompt = PromptWindow::new().expect("prompt window");
    let foreground = unsafe { GetForegroundWindow() };
    prompt.show(PromptRequest::meeting_question(7));

    match wait_for_prompt() {
        Some(hwnd) => {
            let style = unsafe { GetWindowLongPtrW(hwnd as _, GWL_EXSTYLE) } as u32;
            for (flag, name) in [
                (WS_EX_NOACTIVATE, "WS_EX_NOACTIVATE"),
                (WS_EX_TOPMOST, "WS_EX_TOPMOST"),
                (WS_EX_TOOLWINDOW, "WS_EX_TOOLWINDOW"),
            ] {
                assert!(style & flag != 0, "{name} missing");
            }
            assert!(unsafe { IsWindowVisible(hwnd as _) } != 0);
            assert_eq!(
                unsafe { GetForegroundWindow() },
                foreground,
                "showing the prompt moved the foreground window"
            );
            prompt.cancel(7);
        }
        None => {
            let state = query_notification_state();
            assert!(
                state.is_some_and(|s| s.blocks_prompts()),
                "no prompt although Windows allows notifications ({state:?})"
            );
        }
    }
}

#[test]
fn a_prompt_that_is_moot_is_dropped_not_shown() {
    let prompt = PromptWindow::new().expect("prompt window");
    let mut request = PromptRequest::meeting_question(9);
    request.ttl = Duration::ZERO;
    prompt.show(request);
    let event = prompt
        .events()
        .recv_timeout(Duration::from_secs(3))
        .expect("an event");
    assert_eq!(event, PromptEvent::Dropped { id: 9 });
    assert!(find_prompt().is_none());
}

#[test]
fn a_cancelled_prompt_gives_no_answer() {
    let prompt = PromptWindow::new().expect("prompt window");
    prompt.show(PromptRequest::meeting_end(11, Duration::from_secs(60)));
    prompt.cancel(11);
    while let Ok(event) = prompt.events().recv_timeout(Duration::from_millis(500)) {
        assert!(
            !matches!(
                event,
                PromptEvent::Answered {
                    id: 11,
                    outcome: PromptOutcome::Chosen(_)
                }
            ),
            "{event:?}"
        );
    }
}
