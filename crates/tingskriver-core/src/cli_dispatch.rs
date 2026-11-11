//! Commands run on the core runtime, independently of window visibility.
use crate::{cli::Command, host::HostHandle};

pub async fn execute(host: HostHandle, command: Command) -> Result<(), String> {
    use crate::audio::recording_commands as recording;
    static COMMANDS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = COMMANDS.lock().await;
    let service = if matches!(
        &command,
        Command::ToggleDictation | Command::Cancel | Command::Mode(_)
    ) {
        Some(
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if let Ok(service) = crate::dictation::service::get() {
                        break service;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
            })
            .await
            .map_err(|_| "Diktering blev ikke klar".to_string())?,
        )
    } else {
        None
    };
    match command {
        Command::ToggleDictation => service.ok_or("Diktering er ikke klar")?.toggle_hands_free(),
        Command::Cancel => service.ok_or("Diktering er ikke klar")?.cancel_external(),
        Command::Mode(name) => service
            .ok_or("Diktering er ikke klar")?
            .select_external_mode(&name),
        Command::StartMeeting => Err(crate::cli::RECORDING_UNAVAILABLE.into()),
        Command::StopMeeting => recording::stop_recording(
            host,
            recording::RecordingArgs {
                save_path: String::new(),
            },
        )
        .await
        .map(|_| ()),
    }
}
