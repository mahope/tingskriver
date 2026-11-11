// Keep normal release startup free of a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;

fn main() {
    if tingskriver_app::update::is_hook_invocation(&std::env::args().skip(1).collect::<Vec<_>>()) {
        tingskriver_app::run();
        return;
    }
    // Invalid arguments and `--diagnose` write to the terminal and exit; any
    // other launch lets go of it once startup has settled (`run`).
    tingskriver_app::console::attach();
    let args = args::StartupArgs::from_env();
    if args.diagnose || args.validate_delivery.is_some() {
        if let Some(folder) = args.validate_delivery {
            if folder.is_empty() {
                eprintln!("--valider-aflevering kræver en mappe.");
                std::process::exit(1);
            }
            match tingskriver_core::delivery_validate::validate_delivery(std::path::Path::new(
                &folder,
            )) {
                Ok(()) => std::process::exit(0),
                Err(errors) => {
                    for error in errors {
                        eprintln!("{error}");
                    }
                    std::process::exit(1);
                }
            }
        }
        let report = tingskriver_core::diagnose::Report::collect(
            &tingskriver_core::diagnose::system_probe(),
        );
        let output = if args.json {
            match report.to_json() {
                Ok(json) => json,
                Err(_) => {
                    eprintln!("Diagnosen kunne ikke eksporteres.");
                    std::process::exit(1);
                }
            }
        } else {
            report.to_text()
        };
        println!("{output}");
        std::process::exit(i32::from(report.has_errors()));
    }

    tingskriver_app::run();
}
