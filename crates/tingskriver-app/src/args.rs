//! Command line of the app.

/// What the command line asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupArgs {
    /// `--hidden`: start without a window (start with Windows, tray). A
    /// later launch shows the window through the single-instance signal.
    pub hidden: bool,
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
        for arg in args {
            let arg = arg.into();
            match arg.as_str() {
                "--hidden" | "--minimized" => parsed.hidden = true,
                _ => parsed.unknown.push(arg),
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
    fn no_arguments_open_the_window() {
        assert_eq!(StartupArgs::parse(Vec::<String>::new()), StartupArgs::default());
    }

    #[test]
    fn hidden_starts_without_a_window() {
        assert!(StartupArgs::parse(["--hidden"]).hidden);
        assert!(StartupArgs::parse(["--minimized"]).hidden);
    }

    #[test]
    fn unknown_arguments_are_kept_but_ignored() {
        let args = StartupArgs::parse(["--hidden", "--profil", "test"]);
        assert!(args.hidden);
        assert_eq!(args.unknown, vec!["--profil", "test"]);
    }
}
