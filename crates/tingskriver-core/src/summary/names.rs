//! Preferred names (dictionary words and calendar attendees) for the report and
//! translation and title prompts, and the title made from the finished report.
//!
//! Pure functions; the callers fetch the dictionary and the calendar event.

use crate::calendar::Event;

/// Longest title that is stored, in characters.
pub const MAX_TITLE_CHARS: usize = 160;
/// The model answers this when the report has nothing to name.
pub const EMPTY_SENTINEL: &str = "<EMPTY>";
/// Upper bound for the title answer, in tokens.
pub const TITLE_MAX_TOKENS: u32 = 128;

const MAX_NAMES: usize = 60;
const MAX_NAME_CHARS: usize = 60;

/// Names of the attendees who have not declined. Only names, never addresses.
pub fn attendee_names(event: &Event) -> Vec<String> {
    event
        .attendees
        .iter()
        .filter(|attendee| !attendee.declined)
        .filter_map(|attendee| attendee.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// Cleans a list of names: trimmed, one line, no duplicates (case-insensitive),
/// not absurdly long, and at most 60 of them.
pub fn clean_names<I, S>(names: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let name = name
            .as_ref()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if name.is_empty()
            || name.chars().count() > MAX_NAME_CHARS
            || out
                .iter()
                .any(|seen| seen.to_lowercase() == name.to_lowercase())
        {
            continue;
        }
        out.push(name);
        if out.len() == MAX_NAMES {
            break;
        }
    }
    out
}

/// The "write exactly like this" block for the report and title prompts.
/// Empty when there are no names.
pub fn preferred_names_block(names: &[String]) -> String {
    let names = clean_names(names);
    if names.is_empty() {
        return String::new();
    }
    let list = names
        .iter()
        .map(|name| format!("- {name}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "**PREFERRED NAMES AND TERMS:** When one of these names or terms is mentioned, write it exactly like this, with this spelling and capitalisation. Never respell, translate or inflect them wrongly.\n{list}"
    )
}

// -- title -----------------------------------------------------------------

pub fn title_system_prompt(language: &str, names: &[String]) -> String {
    let mut prompt = format!(
        "You write the title of a meeting from its finished report. Write the title in {language}, as plain text on a single line: no Markdown, no quotes, no trailing full stop and at most {MAX_TITLE_CHARS} characters. Make it very short and specific (a few words). If the report has nothing to name the meeting after, answer exactly {EMPTY_SENTINEL}. Output only the title."
    );
    let names = preferred_names_block(names);
    if !names.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(&names);
    }
    prompt
}

pub fn title_user_prompt(report: &str) -> String {
    format!("<report>\n{report}\n</report>")
}

/// A report with nothing but headings, blanks and "none noted" lines is empty
/// and gets no title.
pub fn report_has_content(markdown: &str) -> bool {
    markdown.lines().map(str::trim).any(|line| {
        let bare = line.trim_matches(|c: char| c == '*' || c == '_' || c == '-' || c == '#');
        !bare.trim().is_empty()
            && !line.starts_with('#')
            && !(line.starts_with("**") && line.ends_with("**"))
            && !line.eq_ignore_ascii_case("none noted in this section.")
    })
}

/// One line, at most 160 characters, plain text. `None` for the sentinel or an
/// empty answer.
pub fn clean_title(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|line| !line.is_empty())?;
    if line.contains(EMPTY_SENTINEL) {
        return None;
    }
    let line = line
        .trim_start_matches('#')
        .trim()
        .trim_matches(|c: char| matches!(c, '*' | '_' | '`' | '"' | '\'' | '“' | '”' | '„'))
        .trim()
        .trim_end_matches(['.', ':'])
        .trim();
    let title = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty()
        || matches!(
            title.to_lowercase().as_str(),
            "<add title here>" | "meeting summary" | "add title here" | "[ai-generated title]"
        )
    {
        return None;
    }
    if title.chars().count() <= MAX_TITLE_CHARS {
        return Some(title);
    }
    let cut: String = title.chars().take(MAX_TITLE_CHARS).collect();
    // Prefer a word boundary when it keeps most of the title.
    let cut = match cut.rfind(' ') {
        Some(at) if cut[..at].chars().count() >= MAX_TITLE_CHARS / 2 => cut[..at].to_string(),
        _ => cut,
    };
    Some(cut.trim_end_matches(['.', ':', ',', ' ']).to_string())
}

/// The title replaces the meeting's name only if it is a real, non-empty
/// answer and the report has content.
pub fn title_for_report(report: &str, raw_title: &str) -> Option<String> {
    if !report_has_content(report) {
        return None;
    }
    clean_title(raw_title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::Attendee;
    use chrono::Utc;

    fn event(attendees: Vec<Attendee>) -> Event {
        Event {
            uid: "u".into(),
            title: "Møde".into(),
            attendees,
            start: Utc::now(),
            end: Utc::now(),
            all_day: false,
        }
    }

    fn attendee(name: Option<&str>, declined: bool) -> Attendee {
        Attendee {
            name: name.map(str::to_string),
            email: "x@example.invalid".into(),
            declined,
        }
    }

    #[test]
    fn attendee_names_skip_declined_nameless_and_addresses() {
        let names = attendee_names(&event(vec![
            attendee(Some("Søren Ærø"), false),
            attendee(Some("Afbud"), true),
            attendee(None, false),
            attendee(Some("  "), false),
        ]));
        assert_eq!(names, vec!["Søren Ærø"]);
    }

    #[test]
    fn names_are_cleaned_and_capped() {
        let cleaned = clean_names(["  Åse   Nørgaard ", "åse nørgaard", "", &"x".repeat(61)]);
        assert_eq!(cleaned, vec!["Åse Nørgaard"]);
        let many: Vec<String> = (0..100).map(|i| format!("Navn{i}")).collect();
        assert_eq!(clean_names(&many).len(), 60);
    }

    #[test]
    fn preferred_names_block_lists_names_exactly() {
        assert_eq!(preferred_names_block(&[]), "");
        let block = preferred_names_block(&["Syv.ai".into(), "Jørgen Brøndum".into()]);
        assert!(block.contains("exactly like this"));
        assert!(block.contains("- Syv.ai\n- Jørgen Brøndum"));
        let prompt = title_system_prompt("Danish", &["Syv.ai".into()]);
        assert!(prompt.contains("- Syv.ai") && prompt.contains("<EMPTY>"));
        assert!(!title_system_prompt("Danish", &[]).contains("PREFERRED"));
    }

    #[test]
    fn titles_are_one_line_and_at_most_160_chars() {
        assert_eq!(
            clean_title("# **Budgetmøde**\nmere"),
            Some("Budgetmøde".into())
        );
        assert_eq!(
            clean_title("\n\n \"Næste skridt.\" "),
            Some("Næste skridt".into())
        );
        assert_eq!(clean_title("<EMPTY>"), None);
        assert_eq!(clean_title("<Add Title here>"), None);
        assert_eq!(clean_title("Meeting Summary"), None);
        assert_eq!(clean_title("Titel <EMPTY>"), None);
        assert_eq!(clean_title("  \n "), None);
        let long = format!("{} ", "æøå".repeat(5)).repeat(30);
        let title = clean_title(&long).unwrap();
        assert!(title.chars().count() <= MAX_TITLE_CHARS);
        assert!(!title.contains('\n'));
        // Exactly 160 is kept whole, 161 is cut.
        assert_eq!(clean_title(&"a".repeat(160)).unwrap().chars().count(), 160);
        assert_eq!(clean_title(&"a".repeat(161)).unwrap().chars().count(), 160);
    }

    #[test]
    fn empty_report_gives_no_title() {
        assert!(!report_has_content(""));
        assert!(!report_has_content(
            "# Titel\n\n**Sammendrag**\n\nNone noted in this section.\n"
        ));
        assert!(report_has_content(
            "# Titel\n\n**Sammendrag**\n\nVi aftalte budgettet.\n"
        ));
        assert_eq!(title_for_report("# T\n\n**A**\n", "Budgetmøde"), None);
        assert_eq!(
            title_for_report("# T\n\n**A**\n\nTekst", "Budgetmøde"),
            Some("Budgetmøde".into())
        );
    }
}
