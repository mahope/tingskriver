//! Tingskriver GPUI shell on top of `tingskriver-core`: main window with
//! sidebar navigation, design tokens with live light and dark mode, bundled
//! Danish fonts, a remembered window position, `--hidden` start and a
//! single-instance signal. The database opens in the background; a failure
//! shows an error page with the log path instead of exiting. See
//! `docs/gpui.md`.
//!
//! Dictation, the tray icon and the global shortcuts run in
//! `tingskriver-host`, outside GPUI's event loop; the app only answers its
//! requests (show the window, quit).

pub mod args;
pub mod assets;
pub mod crash;
pub mod probe;
pub mod single_instance;
pub mod startup;
pub mod theme;
pub mod ui;
pub mod window_state;

use gpui::{App, AppContext, QuitMode};
use tingskriver_core::host::AppHost;
use tingskriver_host::{Host, Request};

use crate::args::StartupArgs;
use crate::single_instance::{Instance, Message};
use crate::startup::StartupClock;

/// Log file of the GPUI app in `tingskriver_core::paths::log_dir()`.
pub const LOG_FILE: &str = "tingskriver-app.log";

pub fn run() {
    let clock = StartupClock::start();
    let log_path = tingskriver_core::logging::init(LOG_FILE);
    crash::install_panic_hook(log_path.clone());
    let args = StartupArgs::from_env();
    log::info!(
        "Tingskriver {} (GPUI) starting{}; log file: {}",
        env!("CARGO_PKG_VERSION"),
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

    let message = (!args.hidden).then_some(Message::Show);
    let claimed = if std::env::var_os(single_instance::RESTART_ENV).is_some() {
        runtime.block_on(single_instance::claim_after_restart(
            message,
            std::time::Duration::from_secs(5),
        ))
    } else {
        runtime.block_on(single_instance::claim(message))
    };
    let messages = match claimed {
        Ok(Instance::Primary(messages)) => Some(messages),
        Ok(Instance::Forwarded) => {
            log::info!("Tingskriver already runs; handed over to it");
            return;
        }
        Err(error) => {
            log::warn!("Single-instance check failed, starting anyway: {error}");
            None
        }
    };

    // Tray, shortcuts and dictation, on their own threads. Only the first
    // instance gets here, so there is one keyboard hook per user.
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
    let bridge = host.as_ref().map(|h| h.bridge().clone());
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
                    log::warn!("Unknown page {name:?}; opening Møder");
                }
                page
            });
            let model = cx.new(|_| {
                let mut model = ui::AppModel::new(handle, clock, log_path);
                model.report_first_frame = !args.hidden;
                if let Some(page) = start_page {
                    model.start_page = page;
                }
                model
            });
            if let Some(bridge) = bridge {
                // Core features that need the database get it once it is open.
                cx.observe(&model, move |model, cx| {
                    if let startup::DbState::Ready(db) = &model.read(cx).db {
                        if bridge.database().is_none() {
                            bridge.set_database(Some(db.clone()));
                        }
                    }
                })
                .detach();
            }
            ui::init(model, cx);

            if let Some(mut messages) = messages {
                cx.spawn(async move |cx| {
                    while let Some(message) = messages.recv().await {
                        match message {
                            Message::Show => cx.update(ui::show_main_window),
                        }
                    }
                })
                .detach();
            }

            if let Some(mut requests) = requests {
                cx.spawn(async move |cx| {
                    while let Some(request) = requests.recv().await {
                        match request {
                            Request::ShowWindow => cx.update(ui::show_main_window),
                            // Meetings and the assistant have no GPUI page
                            // yet; open the window where they will be.
                            Request::NewRecording | Request::Ask => {
                                log::info!(
                                    "{request:?} from the tray or a shortcut: opening the window"
                                );
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
        });
    // Remove the tray icon, the shortcuts and the hook before the runtime goes.
    drop(host);
    drop(runtime);
}
