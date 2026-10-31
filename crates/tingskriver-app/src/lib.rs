//! Tingskriver GPUI shell on top of `tingskriver-core`: main window with
//! sidebar navigation, design tokens with live light and dark mode, bundled
//! Danish fonts, a remembered window position, `--hidden` start and a
//! single-instance signal. The database opens in the background; a failure
//! shows an error page with the log path instead of exiting. See
//! `docs/gpui.md`.

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

    let handle = runtime.handle().clone();
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            assets::register_fonts(cx);
            theme::apply(theme::mode_for(cx.window_appearance()), cx);
            // Started hidden (tray, start with Windows), the app keeps
            // running when its window closes; a later launch shows it again.
            cx.set_quit_mode(if args.hidden {
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
    drop(runtime);
}
