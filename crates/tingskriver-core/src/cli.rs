//! Strict, UI-free external command vocabulary. No file, shell or export commands.

#[path = "cli_startup.rs"]
mod startup;
pub use startup::StartupArgs;

pub const RECORDING_UNAVAILABLE: &str = "Mødeoptagelse er deaktiveret: optagestatus er ikke synlig i appen. Meeting recording is disabled: Recording status is not visible in the app.";

/// Printed when a launch cannot hand over to the running instance.
pub const HANDOVER_FAILED: &str = "Tingskriver kunne ikke overdrage til den kørende instans. Tingskriver could not hand over to the running instance.";

/// Printed when the running instance belongs to this account but runs as
/// administrator, so a normal process can neither verify nor reach it.
pub const ELEVATED_INSTANCE: &str = "Tingskriver kører allerede som administrator og kan ikke nås fra en almindelig proces. Luk den, og start den igen uden administratorrettigheder. Tingskriver is already running as administrator and cannot be reached from a normal process. Close it and start it again without administrator rights.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    ToggleDictation,
    Cancel,
    StartMeeting,
    StopMeeting,
    Mode(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Success = 0,
    InvalidInput = 2,
    Transport = 3,
    RecordingUnavailable = 4,
    ElevatedInstance = 5,
}

impl ExitCode {
    /// What to tell whoever started the program, or `None` when the code
    /// needs no sentence of its own.
    pub fn explanation(self) -> Option<&'static str> {
        match self {
            Self::Transport => Some(HANDOVER_FAILED),
            Self::RecordingUnavailable => Some(RECORDING_UNAVAILABLE),
            Self::ElevatedInstance => Some(ELEVATED_INSTANCE),
            Self::Success | Self::InvalidInput => None,
        }
    }
}

impl Command {
    pub fn parse(arg: &str) -> Result<Self, ExitCode> {
        match arg {
            "--toggle-dictation" | "tingskriver://toggle-dictation" => Ok(Self::ToggleDictation),
            "--cancel" | "tingskriver://cancel" => Ok(Self::Cancel),
            "--start-meeting" => Ok(Self::StartMeeting),
            "--stop-meeting" => Ok(Self::StopMeeting),
            _ => {
                let name = arg
                    .strip_prefix("tingskriver://mode/")
                    .ok_or(ExitCode::InvalidInput)?;
                if name.contains(['/', '?', '#', '\\']) {
                    return Err(ExitCode::InvalidInput);
                }
                let mut decoded = Vec::new();
                let mut bytes = name.bytes();
                while let Some(byte) = bytes.next() {
                    decoded.push(if byte == b'%' {
                        let hi = bytes.next().and_then(|b| (b as char).to_digit(16));
                        let lo = bytes.next().and_then(|b| (b as char).to_digit(16));
                        match (hi, lo) {
                            (Some(hi), Some(lo)) => (hi * 16 + lo) as u8,
                            _ => return Err(ExitCode::InvalidInput),
                        }
                    } else {
                        byte
                    });
                }
                let name = String::from_utf8(decoded).map_err(|_| ExitCode::InvalidInput)?;
                if name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control)
                {
                    return Err(ExitCode::InvalidInput);
                }
                Ok(Self::Mode(name))
            }
        }
    }

    pub fn to_argument(&self) -> String {
        match self {
            Self::ToggleDictation => "--toggle-dictation".into(),
            Self::Cancel => "--cancel".into(),
            Self::StartMeeting => "--start-meeting".into(),
            Self::StopMeeting => "--stop-meeting".into(),
            Self::Mode(name) => {
                let encoded: String = name.bytes().map(|b| format!("%{b:02X}")).collect();
                format!("tingskriver://mode/{encoded}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_table() {
        for (arg, command) in [
            ("--toggle-dictation", Command::ToggleDictation),
            ("--cancel", Command::Cancel),
            ("--start-meeting", Command::StartMeeting),
            ("--stop-meeting", Command::StopMeeting),
            ("tingskriver://cancel", Command::Cancel),
            ("tingskriver://mode/Chat", Command::Mode("Chat".into())),
            (
                "tingskriver://mode/M%C3%B8de%20noter",
                Command::Mode("Møde noter".into()),
            ),
            ("tingskriver://mode/C%2B%2B", Command::Mode("C++".into())),
        ] {
            assert_eq!(Command::parse(arg), Ok(command.clone()), "{arg}");
            assert_eq!(Command::parse(&command.to_argument()), Ok(command.clone()));
        }
    }

    #[test]
    fn invalid_input_table() {
        for arg in [
            "--exec",
            "tingskriver://start-meeting",
            "tingskriver://stop-meeting",
            "",
            "https://mode/Chat",
            "tingskriver://mode/",
            "tingskriver://mode/%",
            "tingskriver://mode/%FF",
            "tingskriver://mode/%00",
            "tingskriver://mode/%0A",
            "tingskriver://mode/a/b",
            "tingskriver://mode/a?x=1",
            "tingskriver://mode/a#x",
            "tingskriver://cancel/extra",
            "tingskriver://mode/   ",
        ] {
            assert_eq!(Command::parse(arg), Err(ExitCode::InvalidInput), "{arg}");
        }
    }

    #[test]
    fn documented_exit_codes() {
        for (code, number) in [
            (ExitCode::Success, 0),
            (ExitCode::InvalidInput, 2),
            (ExitCode::Transport, 3),
            (ExitCode::RecordingUnavailable, 4),
            (ExitCode::ElevatedInstance, 5),
        ] {
            assert_eq!(code as i32, number);
        }
    }
}
