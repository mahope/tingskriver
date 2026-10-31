//! Layer 1 cleanup of dictated text (deterministic, no LLM).
//!
//! Passes, in order:
//! 1. voice commands that are the whole dictation ("slet det") and a
//!    trailing "stop diktering";
//! 2. literal words ("skriv ordet komma" → "komma");
//! 3. the user's snippets and dictionary;
//! 4. e-mail and web addresses ([`super::address`]);
//! 5. numbers, dates, times and amounts ([`super::numbers`]);
//! 6. spoken punctuation and symbols ([`super::spoken`]);
//! 7. Danish filler words;
//! 8. the "press Enter" trigger word at the end (chat modes);
//! 9. capital letters at sentence starts, spacing around punctuation.
//!
//! Runs in well under a millisecond for normal dictation lengths. Nothing is
//! ever translated: words the passes don't recognise are kept as spoken,
//! which is what keeps English terms in Danish sentences intact.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::address;
use super::numbers;
use super::spoken::{self, SpokenAction, SpokenTable, VoiceCommand, LITERAL_PREFIXES};
use super::token::{Tok, WordKind};

/// How much the text is touched. `Raw` is used in code editors and terminals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// Fillers removed, sentence starts capitalised.
    #[default]
    Normal,
    /// Only "øh"-type fillers are removed and nothing is capitalised.
    #[serde(alias = "rå")]
    Raw,
}

impl Style {
    pub fn as_str(&self) -> &'static str {
        match self {
            Style::Normal => "normal",
            Style::Raw => "raw",
        }
    }

    pub fn parse(value: &str) -> Style {
        match value.trim().to_lowercase().as_str() {
            "raw" | "rå" => Style::Raw,
            _ => Style::Normal,
        }
    }
}

/// Spoken phrase that is replaced by a fixed spelling ("maho je" → "Mahoje").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replacement {
    pub from: String,
    pub to: String,
    /// The dictionary word is English ("behold engelsk stavning"): its
    /// spelling and case are kept exactly, also at the start of a sentence,
    /// and it is never read as a command, number or sign.
    pub keep_english: bool,
}

/// Spoken trigger that expands to a block of text ("min signatur").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub trigger: String,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Rules {
    pub replacements: Vec<Replacement>,
    pub snippets: Vec<Snippet>,
    /// Spoken punctuation, symbols and commands (built-in plus the user's file).
    pub spoken: SpokenTable,
}

/// Everything that decides how one dictation is cleaned.
#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    pub style: Style,
    pub rules: &'a Rules,
    pub format_numbers: bool,
    pub format_addresses: bool,
    /// Words that press Enter when they end the dictation (chat modes).
    pub enter_triggers: &'a [String],
}

impl<'a> Options<'a> {
    /// Defaults for a style: formatting on, except in the raw style.
    pub fn new(style: Style, rules: &'a Rules) -> Options<'a> {
        Options {
            style,
            rules,
            format_numbers: style == Style::Normal,
            format_addresses: style == Style::Normal,
            enter_triggers: &[],
        }
    }
}

/// The result of layer 1.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cleaned {
    pub text: String,
    /// A voice command for the target program instead of (or after) text.
    pub command: Option<VoiceCommand>,
    /// Press Enter after inserting (the trigger word has been removed).
    pub press_enter: bool,
}

/// Fillers removed everywhere (whole words only), in every style.
const HESITATIONS: &[&str] = &[
    "øh", "øhh", "øhhh", "øhm", "øhmm", "øhhm", "æh", "æhh", "æhm", "ehm", "ehh",
];

/// Fillers removed only at the start of a sentence, and only in `Normal`.
const SENTENCE_FILLERS: &[&str] = &["altså"];

/// Abbreviations whose trailing period does not end a sentence.
const ABBREVIATIONS: &[&str] = &[
    "f.eks", "fx", "bl.a", "ca", "dvs", "mht", "osv", "kl", "nr", "evt", "inkl", "ekskl", "mv",
    "pga", "vedr", "jf", "ift", "hhv", "tlf", "etc", "mr", "hr", "fr", "kr", "pct", "mio", "mia",
];

const TRAILING_PUNCT: &[char] = &['.', ',', '?', '!', ':', ';'];

#[derive(Clone, Debug)]
enum Action {
    Spoken(SpokenAction),
    /// Fixed text. `drop_following_punct` is set for snippets, whose trigger
    /// usually gets a period from the speech model.
    Text {
        text: String,
        kind: WordKind,
        drop_following_punct: bool,
    },
}

struct Matcher {
    words: Vec<String>,
    action: Action,
}

/// Clean dictated text according to `style` and the user's `rules`, with
/// number and address formatting as the style's defaults.
pub fn clean(input: &str, style: Style, rules: &Rules) -> String {
    clean_with(input, &Options::new(style, rules)).text
}

/// Clean dictated text and report voice commands and the Enter trigger.
pub fn clean_with(input: &str, options: &Options) -> Cleaned {
    let mut tokens = tokenize(input);
    let mut command = None;
    if let Some(c) = whole_command(&tokens, &options.rules.spoken) {
        return Cleaned {
            text: String::new(),
            command: Some(c),
            press_enter: false,
        };
    }
    if let Some(cut) = trailing_stop(&tokens, &options.rules.spoken) {
        tokens.truncate(cut);
        while matches!(tokens.last(), Some(Tok::Punct(',' | ';' | ':'))) {
            tokens.pop();
        }
        command = Some(VoiceCommand::StopDictation);
    }

    let tokens = apply_literals(tokens, &options.rules.spoken);
    let tokens = apply_matchers(tokens, &user_matchers(options.rules));
    let tokens = if options.format_addresses {
        address::format(tokens)
    } else {
        tokens
    };
    let tokens = if options.format_numbers {
        numbers::format(tokens)
    } else {
        tokens
    };
    let tokens = apply_matchers(tokens, &spoken_matchers(&options.rules.spoken));
    let tokens = remove_fillers(tokens, options.style);
    let (tokens, press_enter) = take_enter_trigger(tokens, options.enter_triggers);
    let tokens = if options.style == Style::Normal {
        capitalize(tokens)
    } else {
        tokens
    };
    let text = render(&tokens);
    Cleaned {
        press_enter: press_enter && !text.is_empty(),
        text,
        command,
    }
}

/// Words sent to the speech model as a hint (dictionary words and their spelling).
pub fn prompt_words(rules: &Rules) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for r in &rules.replacements {
        let to = r.to.trim();
        if !to.is_empty() && !words.iter().any(|w| w == to) {
            words.push(to.to_string());
        }
    }
    words
}

/// Dictionary words marked as English, for the polishing prompt.
pub fn english_terms(rules: &Rules) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for r in rules.replacements.iter().filter(|r| r.keep_english) {
        let to = r.to.trim();
        if !to.is_empty() && !words.iter().any(|w| w == to) {
            words.push(to.to_string());
        }
    }
    words
}

fn normalize(word: &str) -> String {
    word.to_lowercase()
}

fn phrase_words(phrase: &str) -> Vec<String> {
    phrase
        .split_whitespace()
        .map(|w| normalize(w.trim_matches(|c| TRAILING_PUNCT.contains(&c))))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Ways a phrase can arrive from the speech model: "snabel-a" as one word
/// or as "snabel a".
fn phrase_variants(phrase: &str) -> Vec<Vec<String>> {
    let whole = phrase_words(phrase);
    let split = spoken::phrase_words(phrase);
    if split == whole || split.is_empty() {
        vec![whole]
    } else {
        vec![whole, split]
    }
}

fn user_matchers(rules: &Rules) -> Vec<Matcher> {
    let mut matchers = Vec::new();
    for s in &rules.snippets {
        if s.text.is_empty() {
            continue;
        }
        for words in phrase_variants(&s.trigger) {
            if !words.is_empty() {
                matchers.push(Matcher {
                    words,
                    action: Action::Text {
                        text: s.text.clone(),
                        kind: WordKind::Exact,
                        drop_following_punct: true,
                    },
                });
            }
        }
    }
    for r in &rules.replacements {
        let to = r.to.trim();
        if to.is_empty() {
            continue;
        }
        for words in phrase_variants(&r.from) {
            if !words.is_empty() {
                matchers.push(Matcher {
                    words,
                    action: Action::Text {
                        text: to.to_string(),
                        kind: if r.keep_english {
                            WordKind::Exact
                        } else {
                            WordKind::Fixed
                        },
                        drop_following_punct: false,
                    },
                });
            }
        }
    }
    // Longest phrase wins; the sort is stable so snippets win ties.
    matchers.sort_by_key(|m| std::cmp::Reverse(m.words.len()));
    matchers
}

fn spoken_matchers(table: &SpokenTable) -> Vec<Matcher> {
    let mut matchers = Vec::new();
    for entry in &table.entries {
        // Commands are handled before the other passes.
        if matches!(entry.action, SpokenAction::Command { .. }) {
            continue;
        }
        for phrase in &entry.phrases {
            for words in phrase_variants(phrase) {
                if !words.is_empty() {
                    matchers.push(Matcher {
                        words,
                        action: Action::Spoken(entry.action.clone()),
                    });
                }
            }
        }
    }
    matchers.sort_by_key(|m| std::cmp::Reverse(m.words.len()));
    matchers
}

fn tokenize(input: &str) -> Vec<Tok> {
    let mut tokens = Vec::new();
    for (line_index, line) in input.split('\n').enumerate() {
        if line_index > 0 {
            push_break(&mut tokens, 1);
        }
        for piece in line.split_whitespace() {
            let lead_end = piece
                .char_indices()
                .find(|(_, c)| !TRAILING_PUNCT.contains(c))
                .map(|(i, _)| i)
                .unwrap_or(piece.len());
            let (lead, rest) = piece.split_at(lead_end);
            for c in lead.chars() {
                tokens.push(Tok::Punct(c));
            }
            let core = rest.trim_end_matches(|c| TRAILING_PUNCT.contains(&c));
            if !core.is_empty() {
                tokens.push(Tok::plain(core));
            }
            for c in rest[core.len()..].chars() {
                tokens.push(Tok::Punct(c));
            }
        }
    }
    tokens
}

/// Plain words without hesitations, with the token index of each.
fn spoken_words(tokens: &[Tok]) -> Vec<(usize, String)> {
    tokens
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.plain_word().map(|w| (i, w)))
        .filter(|(_, w)| !HESITATIONS.contains(&w.as_str()))
        .collect()
}

fn command_phrases(table: &SpokenTable) -> Vec<(Vec<String>, VoiceCommand)> {
    let mut out = Vec::new();
    for entry in &table.entries {
        if let SpokenAction::Command { command } = entry.action {
            for phrase in &entry.phrases {
                for words in phrase_variants(phrase) {
                    out.push((words, command));
                }
            }
        }
    }
    out
}

/// "slet det" and "vælg det" only count when they are all that was said.
fn whole_command(tokens: &[Tok], table: &SpokenTable) -> Option<VoiceCommand> {
    let words: Vec<String> = spoken_words(tokens).into_iter().map(|(_, w)| w).collect();
    command_phrases(table)
        .into_iter()
        .find(|(phrase, _)| *phrase == words)
        .map(|(_, command)| command)
}

/// Where a trailing "stop diktering" starts, so the text before it is kept.
fn trailing_stop(tokens: &[Tok], table: &SpokenTable) -> Option<usize> {
    let words = spoken_words(tokens);
    command_phrases(table)
        .into_iter()
        .filter(|(_, c)| *c == VoiceCommand::StopDictation)
        .find_map(|(phrase, _)| {
            let n = phrase.len();
            if n == 0 || words.len() < n {
                return None;
            }
            let tail = &words[words.len() - n..];
            tail.iter()
                .map(|(_, w)| w)
                .eq(phrase.iter())
                .then(|| tail[0].0)
        })
}

/// "skriv ordet komma" → the word "komma"; "skriv ordet ny linje" → "ny linje".
fn apply_literals(tokens: Vec<Tok>, table: &SpokenTable) -> Vec<Tok> {
    let prefixes: Vec<Vec<String>> = LITERAL_PREFIXES.iter().map(|p| phrase_words(p)).collect();
    let phrases = table.phrases();
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let prefix_len = prefixes
            .iter()
            .find(|p| {
                p.iter()
                    .enumerate()
                    .all(|(k, w)| tokens.get(i + k).and_then(Tok::plain_word).as_ref() == Some(w))
            })
            .map(Vec::len);
        let Some(prefix_len) = prefix_len else {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        };
        // The model may put a comma after "skriv ordet".
        let mut start = i + prefix_len;
        while matches!(tokens.get(start), Some(Tok::Punct(',' | ':'))) {
            start += 1;
        }
        let run: Vec<String> = tokens[start.min(tokens.len())..]
            .iter()
            .map_while(Tok::plain_word)
            .collect();
        if run.is_empty() {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        }
        let len = phrases
            .iter()
            .map(|(w, _)| w)
            .find(|w| run.starts_with(w))
            .map(|w| w.len())
            .unwrap_or(1);
        let text: Vec<String> = tokens[start..start + len]
            .iter()
            .filter_map(|t| match t {
                Tok::Word { text, .. } => Some(text.to_lowercase()),
                _ => None,
            })
            .collect();
        out.push(Tok::exact(text.join(" ")));
        i = start + len;
    }
    out
}

/// Matchers grouped by their first word, longest phrase first.
fn index_matchers(matchers: &[Matcher]) -> HashMap<&str, Vec<&Matcher>> {
    let mut index: HashMap<&str, Vec<&Matcher>> = HashMap::new();
    for m in matchers {
        if let Some(first) = m.words.first() {
            index.entry(first.as_str()).or_default().push(m);
        }
    }
    index
}

/// `words` holds the lower-cased plain word of each token (see [`Tok::plain_word`]).
fn match_at<'m>(
    words: &[Option<String>],
    start: usize,
    index: &HashMap<&str, Vec<&'m Matcher>>,
) -> Option<&'m Matcher> {
    let first = words[start].as_deref()?;
    index.get(first)?.iter().copied().find(|m| {
        start + m.words.len() <= words.len()
            && m.words
                .iter()
                .enumerate()
                .all(|(offset, word)| words[start + offset].as_ref() == Some(word))
    })
}

fn quote_is_open(out: &[Tok]) -> bool {
    out.iter()
        .filter(|t| matches!(t, Tok::Sym { text, .. } if text == "\""))
        .count()
        % 2
        == 1
}

fn apply_matchers(tokens: Vec<Tok>, matchers: &[Matcher]) -> Vec<Tok> {
    let words: Vec<Option<String>> = tokens.iter().map(Tok::plain_word).collect();
    let index = index_matchers(matchers);
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let matched = match &tokens[i] {
            Tok::Word { .. } => match_at(&words, i, &index),
            _ => None,
        };
        let Some(m) = matched else {
            match &tokens[i] {
                Tok::Punct(c) => push_punct(&mut out, *c),
                Tok::Break(n) => push_break(&mut out, *n),
                other => out.push(other.clone()),
            }
            i += 1;
            continue;
        };

        i += m.words.len();
        let drop_following = match &m.action {
            Action::Spoken(SpokenAction::Punct { char }) => {
                push_punct(&mut out, *char);
                true
            }
            Action::Spoken(SpokenAction::Break { lines }) => {
                // A comma the speech model put before "ny linje" is noise.
                while matches!(out.last(), Some(Tok::Punct(',' | ';'))) {
                    out.pop();
                }
                push_break(&mut out, *lines);
                true
            }
            Action::Spoken(SpokenAction::Symbol {
                text,
                space_before,
                space_after,
            }) => {
                // "bindestreg" right after a comma from the model: drop the comma.
                if !*space_before {
                    while matches!(out.last(), Some(Tok::Punct(','))) {
                        out.pop();
                    }
                }
                out.push(Tok::sym(text, *space_before, *space_after));
                // Commas the model put after an opening sign are noise.
                !*space_after
            }
            Action::Spoken(SpokenAction::Quote) => {
                if quote_is_open(&out) {
                    out.push(Tok::sym("\"", false, true));
                    false
                } else {
                    out.push(Tok::sym("\"", true, false));
                    true
                }
            }
            Action::Spoken(SpokenAction::Command { .. }) => false,
            Action::Text {
                text,
                kind,
                drop_following_punct,
            } => {
                out.push(Tok::Word {
                    text: text.clone(),
                    kind: *kind,
                });
                *drop_following_punct
            }
        };
        if drop_following {
            while matches!(tokens.get(i), Some(Tok::Punct(_))) {
                i += 1;
            }
        }
    }
    out
}

fn punct_rank(c: char) -> u8 {
    match c {
        ',' => 1,
        ';' | ':' => 2,
        '.' => 3,
        '!' | '?' => 4,
        _ => 0,
    }
}

fn push_punct(out: &mut Vec<Tok>, c: char) {
    match out.last() {
        // Punctuation can't start the text or a line.
        None | Some(Tok::Break(_)) => {}
        Some(Tok::Punct(prev)) => {
            if punct_rank(c) >= punct_rank(*prev) {
                out.pop();
                out.push(Tok::Punct(c));
            }
        }
        // Nothing to attach to after an opening bracket or quote.
        Some(Tok::Sym {
            space_after: false,
            space_before: true,
            ..
        }) => {}
        Some(Tok::Word { .. } | Tok::Sym { .. }) => out.push(Tok::Punct(c)),
    }
}

fn push_break(out: &mut Vec<Tok>, n: u8) {
    if let Some(Tok::Break(prev)) = out.last_mut() {
        *prev = (*prev + n).min(2);
    } else {
        out.push(Tok::Break(n.min(2)));
    }
}

fn is_opening(tok: &Tok) -> bool {
    matches!(
        tok,
        Tok::Sym {
            space_before: true,
            space_after: false,
            ..
        }
    )
}

fn is_sentence_start(out: &[Tok]) -> bool {
    let end = out
        .iter()
        .rposition(|t| !is_opening(t))
        .map(|p| p + 1)
        .unwrap_or(0);
    let out = &out[..end];
    match out.last() {
        None | Some(Tok::Break(_)) => true,
        Some(Tok::Punct(c)) => matches!(c, '.' | '?' | '!') && !ends_with_abbreviation(out),
        Some(Tok::Word { .. } | Tok::Sym { .. }) => false,
    }
}

fn ends_with_abbreviation(out: &[Tok]) -> bool {
    if !matches!(out.last(), Some(Tok::Punct('.'))) || out.len() < 2 {
        return false;
    }
    match &out[out.len() - 2] {
        Tok::Word { text, .. } => ABBREVIATIONS.contains(&normalize(text).as_str()),
        _ => false,
    }
}

fn remove_fillers(tokens: Vec<Tok>, style: Style) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        let is_filler = match tok.plain_word() {
            Some(w) => {
                HESITATIONS.contains(&w.as_str())
                    || (style == Style::Normal
                        && SENTENCE_FILLERS.contains(&w.as_str())
                        && is_sentence_start(&out))
            }
            None => false,
        };
        if is_filler {
            i += 1;
            // "Øh, jeg ..." – the comma belonged to the filler.
            while matches!(tokens.get(i), Some(Tok::Punct(',' | ';'))) {
                i += 1;
            }
            continue;
        }
        match tok {
            Tok::Punct(c) => push_punct(&mut out, *c),
            Tok::Break(n) => push_break(&mut out, *n),
            other => out.push(other.clone()),
        }
        i += 1;
    }
    out
}

/// Remove an Enter trigger word ("send") that ends the dictation. In the
/// middle of the text it is an ordinary word.
fn take_enter_trigger(mut tokens: Vec<Tok>, triggers: &[String]) -> (Vec<Tok>, bool) {
    if triggers.is_empty() {
        return (tokens, false);
    }
    let Some(last_word) = tokens.iter().rposition(|t| !matches!(t, Tok::Punct(_))) else {
        return (tokens, false);
    };
    let mut phrases: Vec<Vec<String>> = triggers
        .iter()
        .map(|t| phrase_words(t))
        .filter(|w| !w.is_empty())
        .collect();
    phrases.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for phrase in phrases {
        let n = phrase.len();
        if last_word + 1 < n {
            continue;
        }
        let start = last_word + 1 - n;
        let matches = phrase
            .iter()
            .enumerate()
            .all(|(k, w)| tokens[start + k].plain_word().as_ref() == Some(w));
        if !matches {
            continue;
        }
        tokens.truncate(start);
        while matches!(tokens.last(), Some(Tok::Punct(',' | ';' | ':'))) {
            tokens.pop();
        }
        return (tokens, true);
    }
    (tokens, false)
}

fn capitalize(tokens: Vec<Tok>) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    for tok in tokens {
        let at_start = is_sentence_start(&out);
        match tok {
            Tok::Word { text, kind } if at_start && kind != WordKind::Exact => {
                out.push(Tok::Word {
                    text: upper_first(&text),
                    kind,
                })
            }
            other => out.push(other),
        }
    }
    out
}

fn upper_first(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn render(tokens: &[Tok]) -> String {
    let mut out = String::new();
    let mut need_space = false;
    for tok in tokens {
        match tok {
            Tok::Word { text, .. } => {
                if need_space {
                    out.push(' ');
                }
                out.push_str(text);
                need_space = true;
            }
            Tok::Punct(c) => {
                // "5 kr." at the end of a sentence gets one period.
                if !(*c == '.' && out.ends_with('.')) {
                    out.push(*c);
                }
                need_space = true;
            }
            Tok::Sym {
                text,
                space_before,
                space_after,
            } => {
                if need_space && *space_before {
                    out.push(' ');
                }
                out.push_str(text);
                need_space = *space_after;
            }
            Tok::Break(n) => {
                while out.ends_with(' ') {
                    out.pop();
                }
                for _ in 0..*n {
                    out.push('\n');
                }
                need_space = false;
            }
        }
    }
    out.trim_end_matches(' ')
        .trim_start_matches(' ')
        .to_string()
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
