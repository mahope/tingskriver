//! Command line of the app.

/// What the command line asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupArgs {
    /// `--hidden`: start without a window (start with Windows, tray). A
    /// later launch shows the window through the single-instance signal.
    pub hidden: bool,
    pub diagnose: bool,
    pub validate_delivery: Option<String>,
    pub json: bool,
    /// `--page meetings|dictation|settings`: the page the window opens on
    /// (manual tests and screenshots).
    pub page: Option<String>,
    /// Arguments that were not understood; logged, never fatal.
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
                    None => parsed.unknown.push(arg),
                },
            }
        }
        parsed
    }

    pub fn from_env() -> Self {
        Self::parse(std::env::args().skip(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn unknown_arguments_are_kept_but_ignored() {
        let args = StartupArgs::parse(["--hidden", "--profil", "test"]);
        assert!(args.hidden);
        assert_eq!(args.unknown, vec!["--profil", "test"]);
    }
}
