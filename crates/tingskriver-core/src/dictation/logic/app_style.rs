//! Cleanup style per program, chosen from the foreground process name.

use super::cleanup::Style;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppStyleRule {
    /// Executable name, e.g. "Code.exe" or "pwsh". Compared case-insensitively
    /// and with or without ".exe".
    pub process: String,
    pub style: Style,
}

/// Code editors and terminals get raw text.
const RAW_PROGRAMS: &[&str] = &[
    "Code.exe",
    "Cursor.exe",
    "WindowsTerminal.exe",
    "pwsh.exe",
    "powershell.exe",
    "cmd.exe",
    "idea64.exe",
    "rider64.exe",
    "devenv.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
];

pub fn default_rules() -> Vec<AppStyleRule> {
    RAW_PROGRAMS
        .iter()
        .map(|process| AppStyleRule {
            process: process.to_string(),
            style: Style::Raw,
        })
        .collect()
}

fn base_name(process: &str) -> &str {
    let file = process.rsplit(['\\', '/']).next().unwrap_or(process).trim();
    match file.get(file.len().saturating_sub(4)..) {
        Some(suffix) if suffix.eq_ignore_ascii_case(".exe") => &file[..file.len() - 4],
        _ => file,
    }
}

pub fn same_program(left: &str, right: &str) -> bool {
    same_name(base_name(left), base_name(right))
}

fn same_name(left: &str, right: &str) -> bool {
    if left.is_ascii() && right.is_ascii() {
        left.eq_ignore_ascii_case(right)
    } else {
        left.to_lowercase() == right.to_lowercase()
    }
}

pub fn is_code_editor(process: Option<&str>) -> bool {
    process.is_some_and(|p| {
        let name = base_name(p);
        RAW_PROGRAMS
            .iter()
            .any(|raw| same_name(name, base_name(raw)))
    })
}

/// The style the user's list sets for `process`, if it names it. A program
/// from [`default_rules`] that the user removed from the list counts as
/// normal, so removing VS Code still turns raw text off.
pub fn chosen_style(process: &str, rules: &[AppStyleRule]) -> Option<Style> {
    let name = base_name(process);
    let named = |rules: &[AppStyleRule]| rules.iter().any(|r| same_name(base_name(&r.process), name));
    if named(rules) {
        Some(style_for(Some(process), rules))
    } else if named(&default_rules()) {
        Some(Style::Normal)
    } else {
        None
    }
}

pub fn style_for(process: Option<&str>, rules: &[AppStyleRule]) -> Style {
    let Some(process) = process else {
        return Style::Normal;
    };
    let name = base_name(process);
    rules
        .iter()
        .find(|rule| same_name(base_name(&rule.process), name))
        .map(|rule| rule.style)
        .unwrap_or(Style::Normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editors_and_terminals_are_raw() {
        let rules = default_rules();
        assert_eq!(style_for(Some("Code.exe"), &rules), Style::Raw);
        assert_eq!(style_for(Some("code.EXE"), &rules), Style::Raw);
        assert_eq!(
            style_for(Some("C:\\Program Files\\PowerShell\\7\\pwsh.exe"), &rules),
            Style::Raw
        );
        assert_eq!(style_for(Some("cmd.exe"), &rules), Style::Raw);
        assert_eq!(style_for(Some("idea64.exe"), &rules), Style::Raw);
    }

    #[test]
    fn everything_else_is_normal() {
        let rules = default_rules();
        assert_eq!(style_for(Some("Spark.exe"), &rules), Style::Normal);
        assert_eq!(style_for(Some("ms-teams.exe"), &rules), Style::Normal);
        assert_eq!(style_for(None, &rules), Style::Normal);
    }

    #[test]
    fn rules_without_exe_suffix() {
        let rules = vec![AppStyleRule {
            process: "pwsh".into(),
            style: Style::Raw,
        }];
        assert_eq!(style_for(Some("pwsh.exe"), &rules), Style::Raw);
        assert_eq!(style_for(Some("pwsh"), &rules), Style::Raw);
    }
}
