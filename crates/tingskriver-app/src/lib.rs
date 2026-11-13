//! Tingskriver GPUI shell on top of `tingskriver-core`: main window with
//! sidebar navigation, design tokens with live light and dark mode, bundled
//! Danish fonts, a remembered window position, `--hidden` start and a
//! single-instance signal, installer hooks and updates through Velopack
//! (`update`). The database opens in the background; a failure
//! shows an error page with the log path instead of exiting. See
//! `docs/gpui.md`.
//!
//! Dictation, the tray icon and the global shortcuts run in
//! `tingskriver-host`, outside GPUI's event loop; the app only answers its
//! requests (show the window, quit).

pub mod about;
mod about_dialog;
pub mod args;
pub mod assets;
pub mod console;
pub mod controls;
pub mod crash;
mod measurement;
mod meetings;
mod onboarding;
mod pages;
pub mod probe;
pub mod proposals;
pub mod recording;
mod recovery;
mod sidebar;
pub mod single_instance;
pub mod startup;
mod summary_run;
pub mod theme;
pub mod ui;
pub mod update;
pub mod window_state;

use gpui::{App, AppContext, QuitMode};
use tingskriver_core::host::AppHost;
use tingskriver_host::{Host, Request};

use crate::args::StartupArgs;
use crate::single_instance::{Launch, Message};
use crate::startup::StartupClock;

/// Log file of the GPUI app in `tingskriver_core::paths::log_dir()`.
pub const LOG_FILE: &str = "tingskriver-app.log";

pub fn run() {
    let clock = StartupClock::start();
    let log_path = tingskriver_core::logging::init(LOG_FILE);
    crash::install_panic_hook(log_path.clone());
    // Installer hooks exit here.
    update::handle_installer_events();
    let mut args = StartupArgs::from_env();
    if args.command.is_some() {
        args.hidden = true;
    }
    log::info!(
        "Tingskriver {} (GPUI) starting{}; log file: {}",
        about::VERSION,
        if args.hidden { " hidden" } else { "" },
        log_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".to_string())
    );
    for arg in &args.unknown {
        log::warn!("Ignoring unknown argument {arg:?}");
    }

    // Core runs on this runtime, never on the UI thread.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("tingskriver-core")
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log::error!("Could not start the tokio runtime: {error}");
            std::process::exit(1);
        }
    };
    tingskriver_core::runtime::set_handle(runtime.handle().clone());

    let message = args
        .command
        .clone()
        .map(Message::Command)
        .or_else(|| (!args.hidden).then_some(Message::Show));
    let launched = if std::env::var_os(single_instance::RESTART_ENV).is_some() {
        // The grace period is the retry window here.
        let claimed = runtime.block_on(single_instance::claim_after_restart(
            message.clone(),
            std::time::Duration::from_secs(5),
        ));
        single_instance::settle(claimed, message.as_ref())
    } else {
        runtime.block_on(single_instance::launch(
            &single_instance::LocalTransport,
            message.clone(),
        ))
    };
    let messages = match launched {
        Ok(Launch::Primary(messages)) => Some(messages),
        Ok(Launch::Forwarded) => {
            log::info!("Tingskriver already runs; handed over to it");
            return;
        }
        Ok(Launch::Unforwarded) => {
            log::warn!(
                "Single-instance endpoint is not available to this launch; starting without forwarding"
            );
            None
        }
        // A second instance would mean a second keyboard hook and doubled
        // text, so a launch that cannot settle the question exits.
        Err(code) => {
            match code.explanation() {
                Some(text) => log::error!("{text}"),
                None => log::error!("Startup stopped with exit code {}", code as i32),
            }
            std::process::exit(code as i32);
        }
    };
    // Everything the terminal was borrowed for has been said.
    console::release();
    // Only the sole instance may replace the installed files.
    update::apply_pending_update(messages.is_some());
    update::start_background_checks();

    // Tray, shortcuts and dictation, on their own threads. Only the first
    // instance gets here, so there is one keyboard hook per user; the one
    // exception is a launch that cannot use the endpoint at all (another
    // account holds it, or none can be created), which starts without
    // forwarding.
    let data_dir = startup::app_data_dir(std::env::var_os(startup::DATA_DIR_ENV));
    let (host, requests) = match data_dir {
        Some(dir) => {
            let (host, requests) = Host::start(dir, runtime.handle().clone());
            (Some(host), Some(requests))
        }
        None => {
            log::error!("No application data folder; dictation, tray and shortcuts are off");
            (None, None)
        }
    };
    let command_host = host.as_ref().map(Host::handle);
    let messages = messages.map(|mut messages| {
        let (show, receiver) = tokio::sync::mpsc::unbounded_channel();
        runtime.spawn(async move {
            let execute = |command| {
                let host = command_host.clone();
                async move {
                    if let Some(host) = host {
                        if let Err(error) =
                            tingskriver_core::cli_dispatch::execute(host, command).await
                        {
                            log::warn!("External command failed: {error}");
                        }
                    } else {
                        log::warn!("External command unavailable: no application data folder");
                    }
                }
            };
            if let Some(Message::Command(command)) = message {
                execute(command).await;
            }
            while let Some(message) = messages.recv().await {
                match message {
                    Message::Show => {
                        let _ = show.send(());
                    }
                    Message::Command(command) => execute(command).await,
                }
            }
        });
        receiver
    });
    let bridge = host.as_ref().map(|h| h.bridge().clone());
    // The meeting recorder: the recording screen draws its state.
    let recorder = host.as_ref().map(Host::recorder);
    let delivery = host.as_ref().map(Host::delivery);
    // First start: onboarding fetches the models through core.
    let onboarding_host = host.as_ref().map(|h| (h.handle(), h.bridge().subscribe()));
    let app_host = host.as_ref().map(|h| h.handle());
    let events = bridge.as_ref().map(|bridge| bridge.subscribe());
    // With a tray icon, closing the window hides the app to the tray.
    let keep_running = host.as_ref().is_some_and(Host::has_tray);

    let handle = runtime.handle().clone();
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            assets::register_fonts(cx);
            theme::apply(theme::mode_for(cx.window_appearance()), cx);
            // Started hidden (tray, start with Windows), the app keeps
            // running when its window closes; a later launch shows it again.
            cx.set_quit_mode(if args.hidden || keep_running {
                QuitMode::Explicit
            } else {
                QuitMode::LastWindowClosed
            });

            let start_page = args.page.as_deref().and_then(|name| {
                let page = ui::Page::from_arg(name);
                if page.is_none() {
                    log::warn!("Unknown page {name:?}; opening the home screen");
                }
                page
            });
            let model = cx.new(|_| {
                let mut model = ui::AppModel::new(handle, clock, log_path);
                model.report_first_frame = !args.hidden;
                if let Some(page) = start_page {
                    model.start_page = page;
                }
                model.recorder = recorder.clone();
                model.host = app_host.clone();
                model
            });
            if let Some(mut notices) = recorder.as_ref().and_then(|r| r.take_notices()) {
                cx.spawn(async move |cx| {
                    while let Some(notice) = notices.recv().await {
                        cx.update(|cx| recording::show_notice(notice, cx));
                    }
                })
                .detach();
            }
            if let Some(delivery) = delivery {
                pages::settings_delivery::start(model.clone(), delivery, cx);
            }
            if let Some(bridge) = bridge {
                let mut events = bridge.subscribe();
                let event_model = model.clone();
                cx.spawn(async move |cx| {
                    while let Some(event) = events.recv().await {
                        if event.name == "mcp-action" {
                            if let Ok(value) =
                                serde_json::from_str::<serde_json::Value>(&event.payload_json)
                            {
                                if value["status"] == "started" {
                                    let action = value["action"]
                                        .as_str()
                                        .unwrap_or(tingskriver_core::strings::MCP_ACTION_UNKNOWN)
                                        .to_string();
                                    cx.update(|cx| ui::show_mcp_action(action, cx));
                                }
                            }
                        } else if event.name == "transcription-queue-status" {
                            if let Ok(status) = serde_json::from_str::<
                                tingskriver_core::transcription_queue::QueueEvent,
                            >(&event.payload_json)
                            {
                                let finished = (status.status == "completed")
                                    .then(|| status.meeting_id.clone());
                                event_model.update(cx, |model, cx| {
                                    model.meeting_jobs.retain(|job| job.job_id != status.job_id);
                                    model.meeting_jobs.push(status);
                                    cx.notify();
                                });
                                // "Automatisk opsummering" (Opsummering settings).
                                if let Some(meeting_id) = finished {
                                    let model = event_model.clone();
                                    cx.update(|cx| {
                                        summary_run::auto_summary(model, meeting_id, cx)
                                    });
                                }
                            }
                        }
                    }
                })
                .detach();
                // Core features that need the database get it once it is open.
                cx.observe(&model, move |model, cx| {
                    if let startup::DbState::Ready(db) = &model.read(cx).db {
                        if bridge.database().is_none() {
                            bridge.set_database(Some(db.clone()));
                            // Voiceprints not matched for 90 days (or the
                            // chosen 30/180) are deleted at startup and daily.
                            tingskriver_core::voiceprint::start_maintenance(db.pool().clone());
                        }
                    }
                })
                .detach();
            }
            onboarding::init(model.clone(), onboarding_host, cx);
            ui::init(model, cx);
            // "Opdatering klar" when the background updater has a version.
            about_dialog::watch_updates(cx);

            if let Some(mut events) = events {
                cx.spawn(async move |cx| {
                    while let Some(event) = events.recv().await {
                        if event.name == "recording-interrupted" {
                            match serde_json::from_str(&event.payload_json) {
                                Ok(notice) => cx.update(|cx| ui::set_interrupted(notice, cx)),
                                Err(e) => {
                                    log::warn!("recording-interrupted: unreadable payload: {e}")
                                }
                            }
                        } else if event.name == tingskriver_core::dictation::rerun::COMPARISON_EVENT
                        {
                            match serde_json::from_str(&event.payload_json) {
                                Ok(comparison) => {
                                    cx.update(|cx| ui::show_rerun_comparison(comparison, cx))
                                }
                                Err(error) => log::warn!("Invalid dictation comparison: {error}"),
                            }
                        } else if event.name == tingskriver_core::dictation::HISTORY_CHANGED_EVENT {
                            cx.update(ui::dictation_history_changed);
                        }
                    }
                })
                .detach();
            }

            if let Some(mut messages) = messages {
                cx.spawn(async move |cx| {
                    while let Some(()) = messages.recv().await {
                        cx.update(ui::show_main_window);
                    }
                })
                .detach();
            }

            if let Some(mut requests) = requests {
                cx.spawn(async move |cx| {
                    while let Some(request) = requests.recv().await {
                        match request {
                            Request::ShowAbout => cx.update(ui::show_about),
                            Request::ShowWindow => cx.update(ui::show_main_window),
                            // "Ny optagelse" from the tray or a shortcut
                            // starts the recording; the window shows it.
                            Request::NewRecording => {
                                cx.update(ui::show_main_window);
                                if let Some(recorder) = &recorder {
                                    recorder.start_recording();
                                }
                            }
                            // The assistant has no GPUI page yet; open the
                            // window where it will be.
                            Request::Ask => {
                                log::info!("Ask from the tray or a shortcut: opening the window");
                                cx.update(ui::show_main_window);
                            }
                            Request::Quit => cx.update(|cx| cx.quit()),
                        }
                    }
                })
                .detach();
            }

            if args.hidden {
                log::info!(
                    "startup: running hidden after {} ms",
                    clock.elapsed().as_millis()
                );
            } else {
                ui::show_main_window(cx);
            }
            ui::load_database(cx);
            ui::load_interrupted(cx);
        });
    // Remove the tray icon, the shortcuts and the hook before the runtime goes.
    drop(host);
    drop(runtime);
}
