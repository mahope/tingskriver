mod pixel_compare;

#[cfg(target_os = "linux")]
#[path = "support/linux.rs"]
mod platform;
// Compile the native API adapter on Linux too: GPUI has test-context
// exports gated to macOS which otherwise fail only after a Windows build.
#[path = "support/windows.rs"]
pub mod native;

use std::path::PathBuf;
use tingskriver_app::{
    startup::{DbFailure, DbState},
    ui::Page,
};

fn fixtures() -> Vec<(String, Page, DbState)> {
    // Navigation and screenshots share this inventory; new pages require goldens.
    let mut fixtures: Vec<_> = Page::ALL
        .into_iter()
        .map(|page| {
            (
                format!("{page:?}").to_lowercase(),
                page,
                DbState::NotCreated,
            )
        })
        .collect();
    fixtures.push(("loading".into(), Page::Meetings, DbState::Loading));
    fixtures.push((
        "database-error".into(),
        Page::Meetings,
        DbState::Failed(DbFailure::from_error(
            "fixture: not a database",
            PathBuf::from("fixtures/meeting_minutes.sqlite"),
        )),
    ));
    fixtures
}

fn main() -> anyhow::Result<()> {
    if std::env::args().skip(1).collect::<Vec<_>>() == ["--watchdog-self-test"] {
        native::watchdog_self_test();
    }
    #[cfg(target_os = "linux")]
    {
        platform::run()
    }
    #[cfg(target_os = "windows")]
    {
        native::run()
    }
}

fn minimum_viewport(scale: f32) -> (u32, u32) {
    use tingskriver_app::theme::layout;
    (
        (f32::from(layout::MIN_WINDOW_WIDTH) * scale).round() as u32,
        (f32::from(layout::MIN_WINDOW_HEIGHT) * scale).round() as u32,
    )
}
