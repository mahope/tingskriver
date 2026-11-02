//! Whole-utterance commands and fail-closed tracking of the last insertion.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Delete,
    Select,
    Stop,
}

pub fn parse(input: &str) -> Option<Command> {
    let words = input
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .trim()
        .to_lowercase();
    match words.as_str() {
        "slet det" | "slet dette" | "fortryd det" | "udvisk det" | "kassér det" => {
            Some(Command::Delete)
        }
        "vælg det" | "markér det" | "marker det" => Some(Command::Select),
        "stop diktering"
        | "stop med at diktere"
        | "stop med at lytte"
        | "afbryd stemmeindtastning midlertidigt"
        | "afbryd diktering midlertidigt"
        | "stop stemmeindtastning"
        | "stoppe diktering"
        | "stop lytning"
        | "stop stemmetilstand"
        | "afbryd stemmetilstand midlertidigt" => Some(Command::Stop),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: isize,
    pub text: Vec<u16>,
    pub selection: (usize, usize),
}

pub struct LastInsertion {
    after: Field,
    inserted: (usize, usize),
}

impl LastInsertion {
    pub fn record(before: &Field, after: &Field, text: &str) -> Option<Self> {
        let (start, end) = before.selection;
        let text: Vec<u16> = text
            .replace("\r\n", "\n")
            .replace('\n', "\r\n")
            .encode_utf16()
            .collect();
        if before.id != after.id || start > end || end > before.text.len() || text.is_empty() {
            return None;
        }
        let mut expected = before.text[..start].to_vec();
        expected.extend(&text);
        expected.extend(&before.text[end..]);
        let caret = start + text.len();
        (expected == after.text && after.selection == (caret, caret)).then(|| Self {
            after: after.clone(),
            inserted: (start, caret),
        })
    }

    pub fn range(&self, current: &Field) -> Option<(usize, usize)> {
        (current == &self.after).then_some(self.inserted)
    }

    pub fn selected(&mut self) {
        self.after.selection = self.inserted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(id: isize, text: &str, start: usize, end: usize) -> Field {
        Field {
            id,
            text: text.encode_utf16().collect(),
            selection: (start, end),
        }
    }

    #[test]
    fn windows_commands_are_whole_utterances() {
        for (phrases, expected) in [
            (
                &["slet det", "slet dette", "fortryd det"][..],
                Command::Delete,
            ),
            (
                &["vælg det", "markér det", "marker det"][..],
                Command::Select,
            ),
            (
                &["stop diktering", "stop med at diktere", "stop med at lytte"][..],
                Command::Stop,
            ),
        ] {
            for phrase in phrases {
                assert_eq!(
                    parse(&format!(" {}. ", phrase.to_uppercase())),
                    Some(expected)
                );
            }
        }
        assert_eq!(parse("jeg sagde slet det"), None);
        assert_eq!(parse("skriv ordet slet det"), None);
    }

    #[test]
    fn windows_stop_and_delete_synonyms() {
        for phrase in [
            "afbryd stemmeindtastning midlertidigt",
            "afbryd diktering midlertidigt",
            "stop stemmeindtastning",
            "stoppe diktering",
            "stop lytning",
            "stop stemmetilstand",
            "afbryd stemmetilstand midlertidigt",
        ] {
            assert_eq!(parse(phrase), Some(Command::Stop), "{phrase}");
        }
        for phrase in ["udvisk det", "kassér det"] {
            assert_eq!(parse(phrase), Some(Command::Delete), "{phrase}");
        }
    }

    #[test]
    fn only_last_inserted_range_can_be_deleted_or_selected() {
        let before = field(1, "før efter", 4, 4);
        let after = field(1, "før æ😀 efter", 8, 8);
        let mut last = LastInsertion::record(&before, &after, "æ😀 ").unwrap();
        assert_eq!(last.range(&after), Some((4, 8)));
        let selected = field(1, "før æ😀 efter", 4, 8);
        last.selected();
        assert_eq!(last.range(&selected), Some((4, 8)));
    }

    #[test]
    fn changed_field_text_or_caret_cannot_be_deleted() {
        let before = field(1, "før", 3, 3);
        let after = field(1, "før tekst", 9, 9);
        let last = LastInsertion::record(&before, &after, " tekst").unwrap();
        for current in [
            field(2, "før tekst", 9, 9),
            field(1, "før tekst!", 10, 10),
            field(1, "før tekst", 0, 0),
        ] {
            assert_eq!(last.range(&current), None);
        }
        assert!(LastInsertion::record(&before, &field(2, "før tekst", 9, 9), " tekst").is_none());
        assert!(LastInsertion::record(&before, &after, "forkert").is_none());
    }

    #[test]
    fn replacing_selection_records_only_the_new_text() {
        let before = field(1, "gammel tekst", 0, 6);
        let after = field(1, "ny tekst", 2, 2);
        assert_eq!(
            LastInsertion::record(&before, &after, "ny")
                .unwrap()
                .range(&after),
            Some((0, 2))
        );
    }
}
