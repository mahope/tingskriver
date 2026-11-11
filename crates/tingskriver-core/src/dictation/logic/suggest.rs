//! Compare snapshots, never keystrokes. Callers must supply only the inserted
//! span from the same field, and must not read password fields.
use std::time::Duration;

pub const CORRECTION_WINDOW: Duration = Duration::from_secs(30);
const MAX_WORDS: usize = 256;

/// Changed words from an eligible snapshot. Oversized inputs fail closed to
/// bound the word-level LCS matrix. Full field contents are never retained.
pub fn corrected_words(
    inserted: &str,
    observed: &str,
    elapsed: Duration,
    password: bool,
) -> Vec<String> {
    if password || elapsed > CORRECTION_WINDOW {
        return Vec::new();
    }
    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_alphabetic() && c != '\'' && c != '-')
            .map(|word| word.trim_matches(['\'', '-']))
            .filter(|word| word.chars().any(char::is_alphabetic))
            .take(MAX_WORDS + 1)
            .map(str::to_owned)
            .collect()
    };
    let before = words(inserted);
    let after = words(observed);
    if before.is_empty() || before.len() > MAX_WORDS || after.len() > MAX_WORDS {
        return Vec::new();
    }
    let mut lcs = vec![vec![0; after.len() + 1]; before.len() + 1];
    let before_keys: Vec<_> = before.iter().map(|word| word.to_lowercase()).collect();
    let after_keys: Vec<_> = after.iter().map(|word| word.to_lowercase()).collect();
    for i in (0..before.len()).rev() {
        for j in (0..after.len()).rev() {
            lcs[i][j] = if before_keys[i] == after_keys[j] {
                1 + lcs[i + 1][j + 1]
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut result = Vec::new();
    let (mut gap_i, mut gap_j) = (0, 0);
    loop {
        let at_end = i == before.len() && j == after.len();
        if at_end || (i < before.len() && j < after.len() && before_keys[i] == after_keys[j]) {
            // Unequal gaps can mix replacements and newly typed text. Fail closed.
            if i - gap_i == j - gap_j {
                for word in &after[gap_j..j] {
                    if !result
                        .iter()
                        .any(|existing: &String| existing.to_lowercase() == word.to_lowercase())
                    {
                        result.push(word.clone());
                    }
                }
            }
            if at_end {
                break;
            }
            i += 1;
            j += 1;
            gap_i = i;
            gap_j = j;
        } else if i < before.len() && (j == after.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
            i += 1;
        } else {
            j += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn danish_names_and_word_alignment() {
        assert_eq!(
            corrected_words(
                "Hej Soren og Ase.",
                "Hej Søren og Åse!",
                Duration::from_secs(2),
                false
            ),
            vec!["Søren", "Åse"]
        );
        assert_eq!(
            corrected_words("Hej Søren", "Hej kære Søren", Duration::ZERO, false),
            Vec::<String>::new()
        );
        assert!(corrected_words("Hej Søren!", "Hej, Søren.", Duration::ZERO, false).is_empty());
        assert!(corrected_words("Hej kære Søren", "Hej Søren", Duration::ZERO, false).is_empty());
    }

    #[test]
    fn only_substitutions_produce_suggestions() {
        for (before, after) in [
            ("Hej Søren", "Hej Søren mit kodeord er eksempel"),
            ("Hej Søren", "mit kodeord er eksempel Hej Søren"),
            ("Hej Søren", "Hej mit kodeord er eksempel Søren"),
            ("Hej Soren", "Hej Søren og kære venner"),
        ] {
            assert!(corrected_words(before, after, Duration::ZERO, false).is_empty());
        }
        assert_eq!(
            corrected_words("Hej Soren igen", "Hej Søren igen", Duration::ZERO, false),
            vec!["Søren"]
        );
    }

    #[test]
    fn token_edges_and_case_do_not_create_corrections() {
        assert!(corrected_words("hej søren", "HEJ SØREN", Duration::ZERO, false).is_empty());
        assert!(corrected_words("Hej Søren", "'Hej' -Søren-", Duration::ZERO, false).is_empty());
        assert_eq!(
            corrected_words("Soren", "-'Søren'-", Duration::ZERO, false),
            vec!["Søren"]
        );
        assert_eq!(
            corrected_words("Ann Sofi", "Anne-Sofie O'Neil", Duration::ZERO, false),
            vec!["Anne-Sofie", "O'Neil"]
        );
    }

    #[test]
    fn window_and_password_guard() {
        assert_eq!(
            corrected_words("Soren", "Søren", CORRECTION_WINDOW, false),
            vec!["Søren"]
        );
        assert!(corrected_words(
            "Soren",
            "Søren",
            CORRECTION_WINDOW + Duration::from_nanos(1),
            false
        )
        .is_empty());
        assert!(corrected_words("Soren", "Søren", Duration::ZERO, true).is_empty());
        assert!(corrected_words("", "Søren", Duration::ZERO, false).is_empty());
        assert!(corrected_words(&"ord ".repeat(257), "Søren", Duration::ZERO, false).is_empty());
    }
}
