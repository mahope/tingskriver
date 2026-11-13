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
    ui::{FixtureMeeting, FixtureVariant, Page, SettingsTab},
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
    // One per settings tab; `settings` above is Generelt
    // (docs/screenshots/indstillinger-*.png).
    for tab in SettingsTab::ALL.into_iter().skip(1) {
        fixtures.push((
            format!("settings-{}", tab.slug()),
            Page::Settings,
            DbState::NotCreated,
            FixtureVariant::Settings(tab),
        ));
    }
    // Job 3: the recording screen (docs/screenshots/optagelse-*.png).
    for (name, variant) in [
        ("recording", FixtureVariant::Recording),
        ("recording-paused", FixtureVariant::RecordingPaused),
        (
            "recording-device-switched",
            FixtureVariant::RecordingDeviceSwitched,
        ),
        ("recording-saving", FixtureVariant::RecordingSaving),
        ("recording-finishing", FixtureVariant::RecordingFinishing),
        ("recording-start-error", FixtureVariant::RecordingStartError),
    ] {
        fixtures.push((name.into(), Page::Home, DbState::NotCreated, variant));
    }
    // "Gendan afbrudte møder" (TranscriptRecovery.tsx) over the home screen.
    fixtures.push((
        "recovery-dialog".into(),
        Page::Home,
        DbState::NotCreated,
        FixtureVariant::RecoveryDialog,
    ));
    // First start (docs/screenshots/onboarding-*.png).
    fixtures.push((
        "onboarding-welcome".into(),
        Page::Home,
        DbState::NotCreated,
        FixtureVariant::OnboardingWelcome,
    ));
    fixtures.push((
        "onboarding-model".into(),
        Page::Home,
        DbState::NotCreated,
        FixtureVariant::OnboardingModel,
    ));
    // The meeting document (docs/screenshots/moedeside-*.png).
    for (name, kind) in [
        ("meeting-summary", FixtureMeeting::Summary),
        ("meeting-transcript", FixtureMeeting::Transcript),
        ("meeting-no-summary", FixtureMeeting::NoSummary),
        ("meeting-summary-error", FixtureMeeting::SummaryFailed),
        ("meeting-proposals", FixtureMeeting::Proposals),
    ] {
        fixtures.push((
            name.into(),
            Page::Home,
            DbState::NotCreated,
            FixtureVariant::Meeting(kind),
        ));
    }
    // Diktering without history and with the keyboard test unfolded.
    fixtures.push((
        "dictation-empty".into(),
        Page::Dictation,
        DbState::NotCreated,
        FixtureVariant::DictationEmpty,
    ));
    fixtures.push((
        "dictation-probe".into(),
        Page::Dictation,
        DbState::NotCreated,
        FixtureVariant::DictationProbe,
    ));
    // The update dialog over the home screen (UpdateDialog.tsx); "about"
    // (Page::About) is the About dialog over the home screen.
    fixtures.push((
        "update-available".into(),
        Page::Home,
        DbState::NotCreated,
        FixtureVariant::UpdateAvailable,
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
