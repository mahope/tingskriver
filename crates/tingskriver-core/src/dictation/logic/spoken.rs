//! Spoken punctuation, symbols and voice commands.
//!
//! The built-in table covers the Danish words for punctuation and symbols
//! plus the commands Danes know from Windows voice typing (Win+H). The user
//! can add, change or switch off entries with a JSON file
//! (`dictation_spoken.json` in the app data folder), see [`SpokenTable::with_user`].
//!
//! "slet det" and "vælg det" act on the text inserted by the previous
//! dictation, and only while the same text field still has focus
//! ([`plan_edit`]).

use serde::{Deserialize, Serialize};

/// A command that acts on the target program instead of inserting text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VoiceCommand {
    /// Remove the text the previous dictation inserted.
    DeleteThat,
    /// Select the text the previous dictation inserted.
    SelectThat,
    /// End hands-free dictation. The text before it is still inserted.
    StopDictation,
}

/// What a spoken phrase turns into.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SpokenAction {
    /// Sentence punctuation (`.` `,` `?` `!` `:` `;`): sticks to the word
    /// before it and replaces punctuation the speech model already put there.
    Punct { char: char },
    /// Any other sign. `spaceBefore`/`spaceAfter` say whether it is
    /// separated from its neighbours: `-` has neither, `(` only before.
    Symbol {
        text: String,
        #[serde(default, rename = "spaceBefore")]
        space_before: bool,
        #[serde(default, rename = "spaceAfter")]
        space_after: bool,
    },
    /// Straight double quote that opens or closes depending on how many came before.
    Quote,
    /// One (`1`) or two (`2`, new paragraph) line breaks.
    Break { lines: u8 },
    /// Only recognised when it is the whole dictation (or, for
    /// `stopDictation`, the end of it).
    Command { command: VoiceCommand },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpokenEntry {
    /// Spoken variants, e.g. `["skråstreg", "skrå streg"]`. Matched whole
    /// word, case-insensitive; a hyphen counts as a space.
    pub phrases: Vec<String>,
    pub action: SpokenAction,
    /// In a user file: remove the built-in phrases listed in `phrases`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

/// The phrases that make the next word literal: "skriv ordet komma" gives "komma".
pub const LITERAL_PREFIXES: &[&str] = &["skriv ordet", "skriv ord", "ordet bogstaveligt"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpokenTable {
    pub entries: Vec<SpokenEntry>,
}

/// Version of the user file format.
pub const SPOKEN_FILE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpokenFile {
    version: u32,
    #[serde(default)]
    entries: Vec<SpokenEntry>,
}

fn punct(phrases: &[&str], c: char) -> SpokenEntry {
    entry(phrases, SpokenAction::Punct { char: c })
}

fn sym(phrases: &[&str], text: &str, space_before: bool, space_after: bool) -> SpokenEntry {
    entry(
        phrases,
        SpokenAction::Symbol {
            text: text.to_string(),
            space_before,
            space_after,
        },
    )
}

fn entry(phrases: &[&str], action: SpokenAction) -> SpokenEntry {
    SpokenEntry {
        phrases: phrases.iter().map(|p| p.to_string()).collect(),
        action,
        disabled: false,
    }
}

/// Normalised words of a phrase: lower case, hyphens as spaces.
pub fn phrase_words(phrase: &str) -> Vec<String> {
    phrase
        .split(|c: char| c.is_whitespace() || c == '-')
        .map(|w| {
            w.trim_matches(|c: char| ".,?!:;".contains(c))
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

impl Default for SpokenTable {
    fn default() -> Self {
        SpokenTable::builtin()
    }
}

impl SpokenTable {
    /// The built-in Danish table.
    pub fn builtin() -> SpokenTable {
        use SpokenAction::{Break, Command, Quote};
        use VoiceCommand::*;
        let entries = vec![
            // Sentence punctuation.
            punct(&["punktum"], '.'),
            punct(&["komma"], ','),
            punct(&["kolon"], ':'),
            punct(&["semikolon"], ';'),
            punct(&["spørgsmålstegn"], '?'),
            punct(&["udråbstegn"], '!'),
            // Dashes and slashes.
            sym(&["bindestreg", "binde streg"], "-", false, false),
            sym(&["tankestreg"], "–", true, true),
            sym(&["skråstreg", "skrå streg"], "/", false, false),
            sym(&["omvendt skråstreg", "backslash"], "\\", false, false),
            sym(
                &["understreg", "under streg", "understregning"],
                "_",
                false,
                false,
            ),
            // Brackets and quotes.
            sym(
                &[
                    "parentes start",
                    "start parentes",
                    "begynd parentes",
                    "venstre parentes",
                ],
                "(",
                true,
                false,
            ),
            sym(
                &[
                    "parentes slut",
                    "slut parentes",
                    "luk parentes",
                    "højre parentes",
                ],
                ")",
                false,
                true,
            ),
            sym(
                &["kantet parentes start", "start kantet parentes"],
                "[",
                true,
                false,
            ),
            sym(
                &["kantet parentes slut", "slut kantet parentes"],
                "]",
                false,
                true,
            ),
            sym(
                &["tuborgparentes start", "krøllet parentes start"],
                "{",
                true,
                false,
            ),
            sym(
                &["tuborgparentes slut", "krøllet parentes slut"],
                "}",
                false,
                true,
            ),
            entry(&["anførselstegn", "citationstegn", "gåseøjne"], Quote),
            sym(
                &[
                    "citat start",
                    "start citat",
                    "begynd citat",
                    "anførselstegn start",
                ],
                "\"",
                true,
                false,
            ),
            sym(
                &[
                    "citat slut",
                    "slut citat",
                    "afslut citat",
                    "anførselstegn slut",
                ],
                "\"",
                false,
                true,
            ),
            sym(&["apostrof"], "'", false, false),
            sym(
                &["prik prik prik", "tre prikker", "udeladelsesprikker"],
                "...",
                false,
                true,
            ),
            // Signs.
            sym(&["snabel-a", "snabela"], "@", false, false),
            sym(&["procenttegn", "procent tegn"], "%", true, true),
            sym(&["plustegn", "plus tegn"], "+", true, true),
            sym(&["minustegn", "minus tegn"], "-", true, true),
            sym(&["lighedstegn", "lig med-tegn"], "=", true, true),
            sym(&["gangetegn"], "×", true, true),
            sym(&["hashtag", "havelåge", "nummertegn"], "#", true, false),
            sym(&["og-tegn", "ampersand"], "&", true, true),
            sym(&["stjernetegn", "asterisk"], "*", true, true),
            sym(&["paragraftegn", "paragraf tegn"], "§", true, true),
            sym(&["gradtegn", "grad tegn"], "°", false, true),
            sym(&["dollartegn", "dollar tegn"], "$", true, true),
            sym(&["eurotegn", "euro tegn"], "€", true, true),
            sym(&["pund tegn", "pundtegn"], "£", true, true),
            sym(&["smiley", "glad smiley"], ":-)", true, true),
            sym(&["sur smiley", "trist smiley"], ":-(", true, true),
            // Line breaks: plan words and Win+H words.
            entry(
                &[
                    "ny linje",
                    "ny linie",
                    "linjeskift",
                    "linieskift",
                    "næste linje",
                ],
                Break { lines: 1 },
            ),
            entry(
                &["nyt afsnit", "afsnitsskift", "næste afsnit"],
                Break { lines: 2 },
            ),
            // Commands (Win+H words plus the English ones for mixed users).
            entry(
                &["slet det", "slet dette", "fortryd det", "delete that"],
                Command {
                    command: DeleteThat,
                },
            ),
            entry(
                &["vælg det", "marker det", "markér det", "select that"],
                Command {
                    command: SelectThat,
                },
            ),
            entry(
                &[
                    "stop diktering",
                    "stop dikteringen",
                    "stop med at diktere",
                    "stop dictation",
                ],
                Command {
                    command: StopDictation,
                },
            ),
        ];
        SpokenTable { entries }
    }

    /// Built-in table with the user's JSON file applied on top. A user entry
    /// replaces built-in entries with the same phrases; `disabled` removes them.
    pub fn with_user(json: &str) -> Result<SpokenTable, String> {
        let file: SpokenFile = serde_json::from_str(json)
            .map_err(|e| format!("Listen over talte tegn kan ikke læses: {e}"))?;
        if file.version > SPOKEN_FILE_VERSION {
            return Err(format!(
                "Listen over talte tegn har version {}, denne version af Tingskriver kan læse {}",
                file.version, SPOKEN_FILE_VERSION
            ));
        }
        let mut table = SpokenTable::builtin();
        for user in file.entries {
            let user_phrases: Vec<Vec<String>> =
                user.phrases.iter().map(|p| phrase_words(p)).collect();
            for builtin in &mut table.entries {
                builtin
                    .phrases
                    .retain(|p| !user_phrases.contains(&phrase_words(p)));
            }
            table.entries.retain(|e| !e.phrases.is_empty());
            if !user.disabled && user_phrases.iter().any(|p| !p.is_empty()) {
                table.entries.insert(0, user);
            }
        }
        Ok(table)
    }

    /// The user file format, e.g. for writing an example file.
    pub fn to_user_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&SpokenFile {
            version: SPOKEN_FILE_VERSION,
            entries: self.entries.clone(),
        })
        .map_err(|e| e.to_string())
    }

    /// The action for a phrase, e.g. `action_for("linjeskift")`.
    pub fn action_for(&self, phrase: &str) -> Option<&SpokenAction> {
        let words = phrase_words(phrase);
        self.entries
            .iter()
            .find(|e| e.phrases.iter().any(|p| phrase_words(p) == words))
            .map(|e| &e.action)
    }

    /// Every (words, action) pair, longest phrase first.
    pub fn phrases(&self) -> Vec<(Vec<String>, &SpokenAction)> {
        let mut out: Vec<(Vec<String>, &SpokenAction)> = self
            .entries
            .iter()
            .flat_map(|e| e.phrases.iter().map(move |p| (phrase_words(p), &e.action)))
            .filter(|(w, _)| !w.is_empty())
            .collect();
        out.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        out
    }
}

// ---------------------------------------------------------------------------
// "slet det" / "vælg det"
// ---------------------------------------------------------------------------

/// The text field that had focus: the program, its top-level window and
/// the focused control inside it (window handles as integers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldId {
    pub process: String,
    pub window: isize,
    pub control: isize,
}

/// What the previous dictation inserted, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastInsertion {
    pub text: String,
    pub field: FieldId,
}

/// Keys to send for a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditPlan {
    /// Press Backspace this many times.
    Backspace(usize),
    /// Hold Shift and press Left this many times.
    SelectLeft(usize),
}

/// Characters the cursor has to move over to cover `text` (a line break
/// counts once, also when the program stores it as CR LF).
pub fn caret_length(text: &str) -> usize {
    text.chars().filter(|c| *c != '\r').count()
}

/// Decide what "slet det"/"vælg det" does. Nothing happens unless the
/// previous insertion is known and the same field still has focus.
pub fn plan_edit(
    command: VoiceCommand,
    last: Option<&LastInsertion>,
    current: Option<&FieldId>,
) -> Option<EditPlan> {
    let last = last?;
    let current = current?;
    if *current != last.field {
        return None;
    }
    let n = caret_length(&last.text);
    if n == 0 {
        return None;
    }
    match command {
        VoiceCommand::DeleteThat => Some(EditPlan::Backspace(n)),
        VoiceCommand::SelectThat => Some(EditPlan::SelectLeft(n)),
        VoiceCommand::StopDictation => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_table_has_at_least_thirty_signs() {
        let table = SpokenTable::builtin();
        let signs = table
            .entries
            .iter()
            .filter(|e| {
                matches!(
                    e.action,
                    SpokenAction::Punct { .. } | SpokenAction::Symbol { .. } | SpokenAction::Quote
                )
            })
            .count();
        assert!(signs >= 30, "only {signs} signs");
    }

    #[test]
    fn no_phrase_is_listed_twice() {
        let table = SpokenTable::builtin();
        let mut seen: Vec<Vec<String>> = Vec::new();
        for (words, _) in table.phrases() {
            assert!(!seen.contains(&words), "{words:?} twice");
            seen.push(words);
        }
    }

    #[test]
    fn windows_words_do_the_same_as_plan_words() {
        let table = SpokenTable::builtin();
        // (Win+H word, plan word)
        let pairs = [
            ("linjeskift", "ny linje"),
            ("linieskift", "ny linje"),
            ("næste linje", "ny linje"),
            ("afsnitsskift", "nyt afsnit"),
            ("næste afsnit", "nyt afsnit"),
            ("udråbstegn", "udråbstegn"),
            ("spørgsmålstegn", "spørgsmålstegn"),
            ("komma", "komma"),
            ("punktum", "punktum"),
            ("kolon", "kolon"),
            ("semikolon", "semikolon"),
            ("slet dette", "slet det"),
            ("markér det", "vælg det"),
            ("stop dikteringen", "stop diktering"),
        ];
        for (windows, plan) in pairs {
            assert_eq!(
                table.action_for(windows),
                table.action_for(plan),
                "{windows} vs {plan}"
            );
            assert!(table.action_for(windows).is_some(), "{windows} missing");
        }
        assert_eq!(
            table.action_for("slet det"),
            Some(&SpokenAction::Command {
                command: VoiceCommand::DeleteThat
            })
        );
        assert_eq!(
            table.action_for("vælg det"),
            Some(&SpokenAction::Command {
                command: VoiceCommand::SelectThat
            })
        );
        assert_eq!(
            table.action_for("stop diktering"),
            Some(&SpokenAction::Command {
                command: VoiceCommand::StopDictation
            })
        );
    }

    #[test]
    fn hyphen_counts_as_space() {
        assert_eq!(phrase_words("snabel-a"), vec!["snabel", "a"]);
        let table = SpokenTable::builtin();
        assert_eq!(table.action_for("Snabel A"), table.action_for("snabel-a"));
    }

    #[test]
    fn user_file_overrides_and_disables() {
        let json = r#"{
            "version": 1,
            "entries": [
                { "phrases": ["tankestreg"], "action": { "type": "symbol", "text": "—", "spaceBefore": true, "spaceAfter": true } },
                { "phrases": ["smiley"], "action": { "type": "quote" }, "disabled": true },
                { "phrases": ["pil højre"], "action": { "type": "symbol", "text": "→", "spaceBefore": true, "spaceAfter": true } }
            ]
        }"#;
        let table = SpokenTable::with_user(json).unwrap();
        assert_eq!(
            table.action_for("tankestreg"),
            Some(&SpokenAction::Symbol {
                text: "—".into(),
                space_before: true,
                space_after: true
            })
        );
        assert_eq!(table.action_for("smiley"), None);
        assert!(table.action_for("glad smiley").is_some());
        assert!(table.action_for("pil højre").is_some());
        assert!(table.action_for("punktum").is_some());
    }

    #[test]
    fn user_file_errors_are_reported() {
        assert!(SpokenTable::with_user("{").is_err());
        assert!(SpokenTable::with_user(r#"{"version": 99, "entries": []}"#).is_err());
        assert_eq!(
            SpokenTable::with_user(r#"{"version": 1}"#).unwrap(),
            SpokenTable::builtin()
        );
    }

    #[test]
    fn user_json_round_trips() {
        let table = SpokenTable::builtin();
        let json = table.to_user_json().unwrap();
        assert_eq!(
            SpokenTable::with_user(&json).unwrap().phrases().len(),
            table.phrases().len()
        );
    }

    fn field(control: isize) -> FieldId {
        FieldId {
            process: "notepad.exe".into(),
            window: 10,
            control,
        }
    }

    #[test]
    fn delete_that_only_in_the_same_field() {
        let last = LastInsertion {
            text: "Hej Anna.".into(),
            field: field(1),
        };
        assert_eq!(
            plan_edit(VoiceCommand::DeleteThat, Some(&last), Some(&field(1))),
            Some(EditPlan::Backspace(9))
        );
        // Another field in the same window.
        assert_eq!(
            plan_edit(VoiceCommand::DeleteThat, Some(&last), Some(&field(2))),
            None
        );
        // Another program.
        let other = FieldId {
            process: "Spark.exe".into(),
            ..field(1)
        };
        assert_eq!(
            plan_edit(VoiceCommand::DeleteThat, Some(&last), Some(&other)),
            None
        );
        // Focus unknown or nothing inserted yet.
        assert_eq!(plan_edit(VoiceCommand::DeleteThat, Some(&last), None), None);
        assert_eq!(
            plan_edit(VoiceCommand::DeleteThat, None, Some(&field(1))),
            None
        );
    }

    #[test]
    fn select_that_and_line_breaks() {
        let last = LastInsertion {
            text: "a\r\nb\nc".into(),
            field: field(1),
        };
        assert_eq!(
            plan_edit(VoiceCommand::SelectThat, Some(&last), Some(&field(1))),
            Some(EditPlan::SelectLeft(5))
        );
        assert_eq!(
            plan_edit(VoiceCommand::StopDictation, Some(&last), Some(&field(1))),
            None
        );
    }
}
