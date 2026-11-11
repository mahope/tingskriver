//! Safety nets around the report model: a minimum size, a length policy and an
//! early check that the answer follows the template.
//!
//! Everything here is pure, so the limits are covered by table tests. The
//! ideas (check the start of the answer and retry once with feedback, cap the
//! length by the transcript, refuse tiny inputs) are reimplemented for our own
//! templates and Danish text; no code is copied.

use super::templates::Template;
use std::future::Future;

/// A transcript with fewer words than this gets no report.
pub const MIN_WORDS: usize = 5;
/// A transcript with fewer characters than this gets no report.
pub const MIN_CHARS: usize = 160;
/// Shown in the UI when a meeting is below the limit. Also used to recognise
/// the case (`is_too_short_message`).
pub const TOO_SHORT_MESSAGE: &str =
    "For kort til referat: der er for lidt tale i mødet til, at et referat giver mening.";

pub fn is_too_short_message(message: &str) -> bool {
    message == TOO_SHORT_MESSAGE
}

/// The spoken text of a transcript: leading `[timestamp]` markers and blank
/// lines are not counted.
fn spoken_text(transcript: &str) -> String {
    transcript
        .lines()
        .map(|line| {
            let line = line.trim();
            match line.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
                Some((_, rest)) => rest.trim(),
                None => line,
            }
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Number of characters of spoken text, as used by the length policy.
pub fn spoken_chars(transcript: &str) -> usize {
    spoken_text(transcript).chars().count()
}

/// No attempt is made for a transcript below `MIN_WORDS` words or `MIN_CHARS`
/// characters (both must be reached).
pub fn check_eligibility(transcript: &str) -> Result<(), &'static str> {
    let spoken = spoken_text(transcript);
    if spoken.split_whitespace().count() < MIN_WORDS || spoken.chars().count() < MIN_CHARS {
        Err(TOO_SHORT_MESSAGE)
    } else {
        Ok(())
    }
}

// -- length policy ---------------------------------------------------------

/// Smallest cap on the report, whatever the transcript.
const CAP_FLOOR: usize = 320;
/// Extra room per template section, so a long template still fits.
const CAP_PER_SECTION: usize = 150;
/// The report is never asked to be longer than this.
const CAP_CEILING: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LengthPolicy {
    /// Upper limit for the whole report, in characters.
    pub max_chars: usize,
    /// Paragraphs (or list items) per section: smallest and largest.
    pub min_paragraphs: usize,
    pub max_paragraphs: usize,
}

/// The cap follows the transcript (a quarter of its characters), with a floor
/// of 320 characters or 150 per template section, whichever is larger, and a
/// ceiling. The paragraph interval grows with the transcript.
pub fn length_policy(transcript_chars: usize, sections: usize) -> LengthPolicy {
    let floor = CAP_FLOOR.max(CAP_PER_SECTION * sections);
    let max_chars = (transcript_chars / 4)
        .max(floor)
        .min(CAP_CEILING.max(floor));
    let (min_paragraphs, max_paragraphs) = match transcript_chars {
        0..=1_499 => (1, 1),
        1_500..=5_999 => (1, 2),
        6_000..=19_999 => (2, 3),
        _ => (2, 4),
    };
    LengthPolicy {
        max_chars,
        min_paragraphs,
        max_paragraphs,
    }
}

/// The policy as guidance for the model.
pub fn length_guidance(policy: &LengthPolicy) -> String {
    let paragraphs = if policy.max_paragraphs == 1 {
        "one short paragraph or a few list items".to_string()
    } else {
        format!(
            "{} to {} short paragraphs or list items",
            policy.min_paragraphs, policy.max_paragraphs
        )
    };
    format!(
        "**LENGTH:** The whole report must be at most {max} characters. Write {paragraphs} per section. Do not pad: a short meeting gets a short report, and a section with little to say gets one sentence or \"None noted in this section.\"",
        max = policy.max_chars
    )
}

// -- start of the report ---------------------------------------------------

/// Lower-case and keep only letters and digits of any script, so `Næste
/// skridt` and `**næste  skridt:**` compare equal and æøå survive.
pub fn normalize_heading(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitute = previous[j] + usize::from(ca != cb);
            current.push(substitute.min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }
    previous[b.len()]
}

/// Two headings match when their normalised forms differ by at most 30 % of
/// the longer one.
pub fn headings_match(found: &str, expected: &str) -> bool {
    let (found, expected) = (normalize_heading(found), normalize_heading(expected));
    let longest = found.chars().count().max(expected.chars().count());
    if longest == 0 {
        return false;
    }
    levenshtein(&found, &expected) * 10 <= longest * 3
}

/// A way the start of the answer breaks the template. `rank` orders the
/// problems so the better of two attempts can be kept (higher is worse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartProblem {
    pub rank: u8,
    pub feedback: String,
}

fn heading_label(line: &str) -> Option<String> {
    let line = line.trim();
    let label = if line.starts_with('#') {
        line.trim_start_matches('#')
    } else if line.len() > 4 && line.starts_with("**") && line.trim_end().ends_with("**") {
        &line[2..line.len() - 2]
    } else {
        return None;
    };
    Some(label.trim().trim_end_matches(':').trim().to_string())
}

/// Checks that the answer starts with a `# ` title and that the first section
/// is the template's first one. `complete: false` is for a partial answer: a
/// title with no section yet is not judged.
pub fn check_report_start(
    markdown: &str,
    template: &Template,
    complete: bool,
) -> Option<StartProblem> {
    let trimmed = markdown.trim_start();
    if !trimmed.starts_with("# ") {
        return Some(StartProblem {
            rank: 2,
            feedback: "The report must start with a single `# ` title line, followed by the template sections in order.".to_string(),
        });
    }
    let expected = &template.sections.first()?.title;
    let first_section = trimmed
        .lines()
        .skip(1)
        .filter_map(heading_label)
        .find(|label| !label.is_empty());
    match first_section {
        Some(label) if headings_match(&label, expected) => None,
        Some(label) => Some(StartProblem {
            rank: 1,
            feedback: format!(
                "The first section must be \"{expected}\", exactly as in the template, but it was \"{label}\". Keep the template's section titles and order."
            ),
        }),
        None if complete => Some(StartProblem {
            rank: 1,
            feedback: format!(
                "The report has no template sections. After the title, the first section must be \"{expected}\"."
            ),
        }),
        None => None,
    }
}

/// The system prompt for the retry.
pub fn retry_system_prompt(system: &str, feedback: &str) -> String {
    format!("{system}\n\nIMPORTANT: Previous attempt failed. {feedback}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedReport {
    pub markdown: String,
    pub retried: bool,
    /// The kept answer follows the template start.
    pub valid: bool,
}

/// Checks the first answer. If it breaks the template, `regenerate` is called
/// once with the feedback. The better of the two attempts is kept; a usable
/// report is never thrown away because a retry failed or was worse.
pub async fn check_with_one_retry<F, Fut>(
    first: String,
    template: &Template,
    regenerate: F,
) -> CheckedReport
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    let Some(problem) = check_report_start(&first, template, true) else {
        return CheckedReport {
            markdown: first,
            retried: false,
            valid: true,
        };
    };
    match regenerate(problem.feedback.clone()).await {
        Ok(second) if !second.trim().is_empty() => {
            match check_report_start(&second, template, true) {
                None => CheckedReport {
                    markdown: second,
                    retried: true,
                    valid: true,
                },
                Some(second_problem) if second_problem.rank < problem.rank => CheckedReport {
                    markdown: second,
                    retried: true,
                    valid: false,
                },
                Some(_) => CheckedReport {
                    markdown: first,
                    retried: true,
                    valid: false,
                },
            }
        }
        _ => CheckedReport {
            markdown: first,
            retried: true,
            valid: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::templates::TemplateSection;

    fn template(titles: &[&str]) -> Template {
        Template {
            name: "t".into(),
            description: "d".into(),
            sections: titles
                .iter()
                .map(|title| TemplateSection {
                    title: (*title).into(),
                    instruction: "i".into(),
                    format: "paragraph".into(),
                    item_format: None,
                    example_item_format: None,
                })
                .collect(),
        }
    }

    #[test]
    fn eligibility_boundaries() {
        let words = |n: usize, w: &str| vec![w; n].join(" ");
        // 4 words, long: too few words.
        assert!(check_eligibility(&words(4, &"a".repeat(60))).is_err());
        // 5 words but 159 characters: too few characters.
        let text = words(5, &"a".repeat(30)); // 5*30 + 4 = 154
        assert!(check_eligibility(&text).is_err());
        let exactly_159 = format!("{} {}", words(4, &"a".repeat(30)), "a".repeat(35));
        assert_eq!(exactly_159.chars().count(), 159);
        assert!(check_eligibility(&exactly_159).is_err());
        // 5 words and exactly 160 characters is enough.
        let exactly_160 = format!("{} {}", words(4, &"a".repeat(30)), "a".repeat(36));
        assert_eq!(exactly_160.chars().count(), 160);
        assert!(check_eligibility(&exactly_160).is_ok());
        assert!(check_eligibility("").is_err());
        assert!(check_eligibility("  \n \n").is_err());
    }

    #[test]
    fn eligibility_ignores_timestamps_and_counts_unicode_chars() {
        let line = format!("[00:00:01] {}", "æøå ".repeat(41));
        assert!(check_eligibility(&line).is_ok());
        // Timestamps alone do not make a transcript long enough.
        let stamps = "[00:00:01] hej\n".repeat(30);
        assert!(check_eligibility(&stamps).is_err());
        assert_eq!(spoken_chars("[00:00:01] æøå"), 3);
    }

    #[test]
    fn length_policy_table() {
        // (transcript chars, sections, max_chars, min paragraphs, max paragraphs)
        let table = [
            (0, 3, 450, 1, 1),     // floor: 150 per section beats 320
            (160, 1, 320, 1, 1),   // floor 320
            (1_499, 2, 374, 1, 1), // 1499/4 beats the floor
            (1_500, 2, 375, 1, 2),
            (5_999, 5, 1_499, 1, 2),
            (6_000, 5, 1_500, 2, 3),
            (19_999, 4, 4_999, 2, 3),
            (20_000, 4, 5_000, 2, 4),
            (100_000, 4, 10_000, 2, 4),   // ceiling
            (100_000, 100, 15_000, 2, 4), // template floor beats the ceiling
        ];
        for (chars, sections, max, lo, hi) in table {
            assert_eq!(
                length_policy(chars, sections),
                LengthPolicy {
                    max_chars: max,
                    min_paragraphs: lo,
                    max_paragraphs: hi
                },
                "chars={chars} sections={sections}"
            );
        }
    }

    #[test]
    fn length_guidance_mentions_the_limits() {
        let short = length_guidance(&length_policy(200, 3));
        assert!(short.contains("at most 450 characters"));
        assert!(short.contains("one short paragraph"));
        let long = length_guidance(&length_policy(30_000, 3));
        assert!(long.contains("2 to 4 short paragraphs"));
    }

    #[test]
    fn danish_headings_compare_with_unicode_normalisation() {
        assert!(headings_match("Næste skridt", "næste  skridt"));
        assert!(headings_match("**Næste skridt:**", "Næste skridt"));
        assert!(headings_match("Næste skridt", "Næste skrid"));
        // An ASCII-only normaliser would reduce both to "nste skridt"/"ste skridt".
        assert_ne!(normalize_heading("Næste skridt"), "nstesskridt");
        assert_eq!(normalize_heading("Ørsted Å"), "ørstedå");
        assert!(!headings_match("Beslutninger", "Næste skridt"));
        assert!(!headings_match("", ""));
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kat", "kat"), 0);
        assert_eq!(levenshtein("æble", "eble"), 1);
        assert_eq!(levenshtein("flaw", "lawn"), 2);
    }

    #[test]
    fn start_check_cases() {
        let t = template(&["Sammendrag", "Næste skridt"]);
        let ok = "# Budgetmøde\n\n**Sammendrag**\n\nTekst\n\n**Næste skridt**\n";
        assert_eq!(check_report_start(ok, &t, true), None);
        // `##` and trailing colon are fine.
        assert_eq!(check_report_start("# T\n## Sammendrag:\nx", &t, true), None);
        // No title.
        let no_title = check_report_start("**Sammendrag**\nx", &t, true).unwrap();
        assert_eq!(no_title.rank, 2);
        // Wrong first section.
        let wrong = check_report_start("# T\n\n**Beslutninger**\nx", &t, true).unwrap();
        assert_eq!(wrong.rank, 1);
        assert!(wrong.feedback.contains("Sammendrag"));
        // Partial answers: a lone title is not judged yet, a wrong start is.
        assert_eq!(check_report_start("# Bud", &t, false), None);
        assert!(check_report_start("# Bud", &t, true).is_some());
        assert!(check_report_start("Bud", &t, false).is_some());
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
    }

    const GOOD: &str = "# T\n\n**Sammendrag**\n\nx";
    const WRONG_SECTION: &str = "# T\n\n**Noget andet**\n\nx";
    const NO_TITLE: &str = "**Sammendrag**\n\nx";

    #[test]
    fn valid_first_answer_is_not_retried() {
        let result = rt().block_on(check_with_one_retry(
            GOOD.to_string(),
            &template(&["Sammendrag"]),
            |_| async { panic!("must not retry") },
        ));
        assert_eq!(
            result,
            CheckedReport {
                markdown: GOOD.into(),
                retried: false,
                valid: true
            }
        );
    }

    #[test]
    fn wrong_heading_is_retried_once_with_feedback() {
        let seen = std::sync::Mutex::new(Vec::new());
        let result = rt().block_on(check_with_one_retry(
            WRONG_SECTION.to_string(),
            &template(&["Sammendrag"]),
            |feedback| {
                seen.lock().unwrap().push(feedback);
                async { Ok(GOOD.to_string()) }
            },
        ));
        assert_eq!(result.markdown, GOOD);
        assert!(result.retried && result.valid);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].contains("Sammendrag"));
    }

    #[test]
    fn best_attempt_is_kept_when_retry_is_no_better() {
        let t = template(&["Sammendrag"]);
        // Retry is worse (no title): keep the first.
        let worse = rt().block_on(check_with_one_retry(
            WRONG_SECTION.to_string(),
            &t,
            |_| async { Ok(NO_TITLE.to_string()) },
        ));
        assert_eq!(worse.markdown, WRONG_SECTION);
        assert!(worse.retried && !worse.valid);
        // Retry is better but still invalid: keep the retry.
        let better = rt().block_on(check_with_one_retry(NO_TITLE.to_string(), &t, |_| async {
            Ok(WRONG_SECTION.to_string())
        }));
        assert_eq!(better.markdown, WRONG_SECTION);
        // Retry fails or is empty: keep the first.
        for retry in [Err("fejl".to_string()), Ok("  ".to_string())] {
            let kept = rt().block_on(check_with_one_retry(
                WRONG_SECTION.to_string(),
                &t,
                |_| async move { retry },
            ));
            assert_eq!(kept.markdown, WRONG_SECTION);
            assert!(!kept.valid);
        }
    }

    #[test]
    fn retry_prompt_carries_the_feedback() {
        let prompt = retry_system_prompt("SYS", "Fix the heading.");
        assert!(prompt.starts_with("SYS"));
        assert!(prompt.ends_with("IMPORTANT: Previous attempt failed. Fix the heading."));
    }
}
