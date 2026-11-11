//! Command line of the app.

/// What the command line asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupArgs {
    /// `--hidden`: start without a window (start with Windows, tray). A
    /// later launch shows the window through the single-instance signal.
    pub hidden: bool,
    pub command: Option<super::Command>,
    pub diagnose: bool,
    /// `--valider-aflevering <mappe>`: check a delivery folder and exit.
    pub validate_delivery: Option<String>,
    pub json: bool,
    /// `--page meetings|dictation|settings`: the page the window opens on
    /// (manual tests and screenshots).
    pub page: Option<String>,
    /// Arguments rejected by validation before startup.
    pub unknown: Vec<String>,
}

impl StartupArgs {
    /// Parse the arguments after the program name.
    pub fn parse<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut parsed = StartupArgs::default();
        let mut args = args.into_iter().map(Into::into);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--valider-aflevering" | "--validate-delivery" => {
                    parsed.validate_delivery = Some(
                        args.next()
                            .filter(|folder| !folder.starts_with('-'))
                            .unwrap_or_default(),
                    );
                }
                "--diagnose" => parsed.diagnose = true,
                "--json" => parsed.json = true,
                "--hidden" | "--minimized" => parsed.hidden = true,
                "--page" => match args.next() {
                    Some(page) => parsed.page = Some(page),
                    None => parsed.unknown.push(arg),
                },
                _ => match arg.strip_prefix("--page=") {
                    Some(page) => parsed.page = Some(page.to_string()),
                    None => match super::Command::parse(&arg) {
                        Ok(command) if parsed.command.is_none() => {
                            parsed.command = Some(command);
                        }
                        _ => parsed.unknown.push(arg),
                    },
                },
            }
        }
        parsed
    }

    pub fn validate(&self) -> Result<(), super::ExitCode> {
        if !self.unknown.is_empty()
            || (self.json && !self.diagnose)
            || (self.diagnose && (self.command.is_some() || self.hidden || self.page.is_some()))
            || self
                .page
                .as_deref()
                .is_some_and(|page| !matches!(page, "meetings" | "dictation" | "settings"))
            || (self.command.is_some() && self.page.is_some())
        {
            Err(super::ExitCode::InvalidInput)
        } else if self.command == Some(super::Command::StartMeeting) {
            Err(super::ExitCode::RecordingUnavailable)
        } else {
            Ok(())
        }
    }

    pub fn from_env() -> Self {
        Self::from_args(std::env::args().skip(1))
    }

    pub fn from_args(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let parsed = Self::parse(args);
        if let Err(code) = parsed.validate() {
            if code == super::ExitCode::RecordingUnavailable {
                eprintln!("{}", super::RECORDING_UNAVAILABLE);
            } else {
                eprintln!("Ugyldige argumenter. Se docs/kommandolinje.md.");
            }
            std::process::exit(code as i32);
        }
        parsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_process_exit_codes() {
        for (argument, code) in [
            ("--unknown-command", 2),
            ("--start-meeting", 4),
            ("tingskriver://start-meeting", 2),
            ("tingskriver://stop-meeting", 2),
            ("tingskriver://mode/Chat", 0),
        ] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "cli::startup::tests::cli_process_entry",
                    "--nocapture",
                ])
                .env("TINGSKRIVER_TEST_CLI_ARGUMENT", argument)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(code), "{argument}");
            if code == 4 {
                assert!(String::from_utf8_lossy(&output.stderr)
                    .contains("Recording status is not visible"));
            }
        }
    }

    #[test]
    fn cli_process_entry() {
        if let Ok(argument) = std::env::var("TINGSKRIVER_TEST_CLI_ARGUMENT") {
            StartupArgs::from_args([argument]);
        }
    }

    #[test]
    fn delivery_validation_flags() {
        for flag in ["--valider-aflevering", "--validate-delivery"] {
            assert_eq!(
                StartupArgs::parse([flag, "mappe"])
                    .validate_delivery
                    .as_deref(),
                Some("mappe")
            );
            assert_eq!(
                StartupArgs::parse([flag]).validate_delivery.as_deref(),
                Some("")
            );
            for invalid in ["--json", "--hidden", "-x", ""] {
                assert_eq!(
                    StartupArgs::parse([flag, invalid])
                        .validate_delivery
                        .as_deref(),
                    Some("")
                );
            }
        }
    }

    #[test]
    fn diagnose_and_json_are_order_independent() {
        for flags in [["--diagnose", "--json"], ["--json", "--diagnose"]] {
            let args = StartupArgs::parse(flags);
            assert!(args.diagnose && args.json);
            assert!(args.unknown.is_empty());
        }
        assert!(StartupArgs::parse(["--diagnose"]).diagnose);
    }

    #[test]
    fn no_arguments_open_the_window() {
        assert_eq!(
            StartupArgs::parse(Vec::<String>::new()),
            StartupArgs::default()
        );
    }

    #[test]
    fn hidden_starts_without_a_window() {
        assert!(StartupArgs::parse(["--hidden"]).hidden);
        assert!(StartupArgs::parse(["--minimized"]).hidden);
    }

    #[test]
    fn page_can_be_chosen() {
        assert_eq!(
            StartupArgs::parse(["--page", "settings"]).page.as_deref(),
            Some("settings")
        );
        assert_eq!(
            StartupArgs::parse(["--page=dictation"]).page.as_deref(),
            Some("dictation")
        );
        assert_eq!(StartupArgs::parse(["--page"]).unknown, vec!["--page"]);
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        let args = StartupArgs::parse(["--hidden", "--profil", "test"]);
        assert!(args.hidden);
        assert_eq!(args.unknown, vec!["--profil", "test"]);
        assert_eq!(args.validate(), Err(super::super::ExitCode::InvalidInput));
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    #[test]
    fn argument_combinations() {
        for args in [
            vec!["--json"],
            vec!["--page"],
            vec!["--page=unknown"],
            vec!["--cancel", "--cancel"],
            vec!["--cancel", "--diagnose"],
            vec!["--page=settings", "--cancel"],
            vec!["--hidden", "--diagnose"],
        ] {
            assert_eq!(
                StartupArgs::parse(args.clone()).validate(),
                Err(super::super::ExitCode::InvalidInput),
                "{args:?}"
            );
        }
        for args in [
            vec![],
            vec!["--hidden"],
            vec!["--hidden", "--cancel"],
            vec!["--diagnose", "--json"],
            vec!["--page=settings"],
            vec!["tingskriver://mode/M%C3%B8de"],
        ] {
            assert_eq!(
                StartupArgs::parse(args.clone()).validate(),
                Ok(()),
                "{args:?}"
            );
        }
    }
}
