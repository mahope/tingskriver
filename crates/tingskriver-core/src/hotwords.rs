//! Ordered speech hints. No calendar addresses, persistence or network calls.

use serde::{Deserialize, Serialize};

/// Snapshot supplied by the host for the matched meeting, never all calendars.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Hotwords {
    pub participant_names: Vec<String>,
    pub dictionary_words: Vec<String>,
}

impl Hotwords {
    pub fn from_sources(
        event: Option<&crate::calendar::Event>,
        dictionary: &[crate::dictation::store::DictionaryWord],
    ) -> Self {
        Self {
            participant_names: event
                .into_iter()
                .flat_map(|event| &event.attendees)
                .filter(|attendee| !attendee.declined)
                .filter_map(|attendee| attendee.name.clone())
                .collect(),
            dictionary_words: dictionary.iter().map(|word| word.word.clone()).collect(),
        }
    }

    /// A conservative UTF-8 byte budget also bounds byte-level model tokens.
    /// Keep whole names/words; deduplicate ignoring case, retaining the first form.
    pub fn prompt(&self, max_bytes: usize) -> Option<String> {
        let mut selected = Vec::new();
        let mut used = 0;
        let mut seen = std::collections::HashSet::new();
        for word in self.participant_names.iter().chain(&self.dictionary_words) {
            let word = word.trim();
            if word.is_empty() || seen.contains(&word.to_lowercase()) {
                continue;
            }
            let needed = word.len() + if selected.is_empty() { 0 } else { 2 };
            if needed > max_bytes.saturating_sub(used) {
                continue;
            }
            seen.insert(word.to_lowercase());
            selected.push(word);
            used += needed;
        }
        (!selected.is_empty()).then(|| selected.join(", "))
    }
}

/// Whisper-compatible endpoints accept 224 prompt tokens. Using at most 224
/// UTF-8 bytes is deliberately conservative and needs no model tokenizer.
pub const PROMPT_BUDGET: usize = 224;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_use_names_and_only_dictionary_targets() {
        let now = chrono::Utc::now();
        let event = crate::calendar::Event {
            uid: "test".into(),
            title: "test".into(),
            start: now,
            end: now,
            all_day: false,
            attendees: vec![
                crate::calendar::Attendee {
                    name: Some("Ægir".into()),
                    email: "hidden@example.invalid".into(),
                    declined: false,
                },
                crate::calendar::Attendee {
                    name: Some("Declined".into()),
                    email: String::new(),
                    declined: true,
                },
                crate::calendar::Attendee {
                    name: None,
                    email: "unnamed@example.invalid".into(),
                    declined: false,
                },
            ],
        };
        let dictionary = vec![crate::dictation::store::DictionaryWord {
            id: 1,
            word: "Åle".into(),
            aliases: "Aale, Aal".into(),
            english: false,
        }];
        let hints = Hotwords::from_sources(Some(&event), &dictionary);
        assert_eq!(hints.prompt(224).as_deref(), Some("Ægir, Åle"));
        assert_eq!(
            Hotwords::from_sources(None, &dictionary)
                .prompt(224)
                .as_deref(),
            Some("Åle")
        );
    }

    #[test]
    fn priority_preserves_spellings_and_danish_letters() {
        let hints = Hotwords {
            participant_names: vec![" Ægir Ø ".into(), "ægir ø".into()],
            dictionary_words: vec!["Åle".into(), "Aale".into(), "åle".into(), "".into()],
        };
        assert_eq!(hints.prompt(224).as_deref(), Some("Ægir Ø, Åle, Aale"));
        assert_eq!(hints.prompt(8).as_deref(), Some("Ægir Ø"));
    }

    #[test]
    fn budget_includes_separators_and_never_splits_unicode() {
        let hints = Hotwords {
            participant_names: vec!["ø".into()],
            dictionary_words: vec!["å".into(), "x".repeat(225), "a".into()],
        };
        assert_eq!(hints.prompt(0), None);
        assert_eq!(hints.prompt(1).as_deref(), Some("a"));
        assert_eq!(hints.prompt(5).as_deref(), Some("ø, a"));
        assert_eq!(hints.prompt(6).as_deref(), Some("ø, å"));
        assert!(hints.prompt(PROMPT_BUDGET).unwrap().len() <= PROMPT_BUDGET);
        assert_eq!(Hotwords::default().prompt(224), None);
        let exact = Hotwords {
            participant_names: vec!["ø".repeat(112)],
            dictionary_words: vec!["extra".into()],
        };
        assert_eq!(exact.prompt(PROMPT_BUDGET).unwrap().len(), PROMPT_BUDGET);
        assert_eq!(exact.prompt(PROMPT_BUDGET - 1).as_deref(), Some("extra"));
    }
}
