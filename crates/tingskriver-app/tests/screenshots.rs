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
    ui::{FixtureVariant, Page},
};

fn fixtures() -> Vec<(String, Page, DbState, FixtureVariant)> {
    // Every page has a golden (Page::ALL); new pages require goldens.
    let mut fixtures: Vec<_> = Page::ALL
        .into_iter()
        .map(|page| {
            (
                format!("{page:?}").to_lowercase(),
                page,
                DbState::NotCreated,
                FixtureVariant::Plain,
            )
        })
        .collect();
    // The sidebar with meetings (docs/screenshots/moedeliste-*.png).
    fixtures.push((
        "meetings".into(),
        Page::Home,
        DbState::NotCreated,
        FixtureVariant::Meetings,
    ));
    fixtures.push((
        "sidebar-collapsed".into(),
        Page::Home,
        DbState::NotCreated,
        FixtureVariant::SidebarCollapsed,
    ));
    fixtures.push((
        "loading".into(),
        Page::Home,
        DbState::Loading,
        FixtureVariant::Plain,
    ));
    fixtures.push((
        "database-error".into(),
        Page::Home,
        DbState::Failed(DbFailure::from_error(
            "fixture: not a database",
            PathBuf::from("fixtures/meeting_minutes.sqlite"),
        )),
        FixtureVariant::Plain,
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
