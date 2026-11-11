use super::{Source, Window};

pub(super) fn normalized(process: &str) -> String {
    process.to_ascii_lowercase()
}

pub(super) fn browser(process: &str) -> bool {
    matches!(
        process,
        "chrome.exe" | "msedge.exe" | "firefox.exe" | "brave.exe" | "opera.exe" | "vivaldi.exe"
    )
}

pub(super) fn native(source: Source, process: &str) -> bool {
    match source {
        Source::TeamsApp => matches!(process, "teams.exe" | "ms-teams.exe"),
        Source::ZoomApp => matches!(process, "zoom.exe" | "cpthost.exe"),
        Source::MeetApp => matches!(process, "google meet.exe" | "meet.exe") || browser(process),
        _ => false,
    }
}

pub(super) fn microphone_matches(source: Source, window_process: &str, mic: &str) -> bool {
    match source {
        // Zoom may capture through its companion process rather than the window owner.
        Source::ZoomApp => native(source, mic),
        _ => window_process == mic,
    }
}

pub(super) fn source(window: &Window) -> Option<Source> {
    let process = normalized(&window.process_name);
    let title = window.title.to_lowercase();
    let teams = title.ends_with(" | microsoft teams")
        && !matches!(
            title.split(" | ").next(),
            Some("chat" | "activity" | "aktivitet" | "calendar" | "kalender" | "teams")
        );
    let meet = title.strip_prefix("meet - ").is_some_and(|rest| {
        let code = rest.split_whitespace().next().unwrap_or_default();
        code.len() == 12
            && code.bytes().enumerate().all(|(i, b)| {
                if i == 3 || i == 8 {
                    b == b'-'
                } else {
                    b.is_ascii_lowercase()
                }
            })
    }) || title
        .strip_prefix("møde - ")
        .is_some_and(|rest| !rest.trim().is_empty());
    let zoom = title == "zoom meeting"
        || title.starts_with("zoom meeting - ")
        || title == "zoom-møde"
        || title.starts_with("zoom-møde - ");
    if window.is_browser {
        if !browser(&process) {
            return None;
        }
        if teams {
            Some(Source::TeamsBrowser)
        } else if meet {
            Some(Source::MeetBrowser)
        } else if zoom {
            Some(Source::ZoomBrowser)
        } else {
            None
        }
    } else if teams && native(Source::TeamsApp, &process) {
        Some(Source::TeamsApp)
    } else if meet && native(Source::MeetApp, &process) {
        Some(Source::MeetApp)
    } else if zoom && native(Source::ZoomApp, &process) {
        Some(Source::ZoomApp)
    } else {
        None
    }
}
