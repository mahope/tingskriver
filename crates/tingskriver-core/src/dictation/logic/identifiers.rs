//! Pure code-style token formatting, before spoken punctuation is expanded.

use super::cleanup::Style;
use super::spoken::{phrase_words, SpokenTable};
use super::token::Tok;

pub(super) fn format(tokens: Vec<Tok>, style: Style, spoken: &SpokenTable) -> Vec<Tok> {
    if style != Style::Raw {
        return tokens;
    }
    let command_words: std::collections::HashSet<_> = spoken
        .entries
        .iter()
        .flat_map(|entry| entry.phrases.iter().flat_map(|phrase| phrase_words(phrase)))
        .collect();
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let word = tokens[i].plain_word().unwrap_or_default();
        let (case, consumed) = match word.as_str() {
            "camelcase" => ("camel", 1),
            "snake_case" => ("snake", 1),
            "kebab-case" => ("kebab", 1),
            "pascalcase" => ("pascal", 1),
            "camel" | "snake" | "kebab" | "pascal"
                if tokens.get(i + 1).and_then(Tok::plain_word).as_deref() == Some("case") =>
            {
                (word.as_str(), 2)
            }
            _ => ("", 0),
        };
        if consumed > 0 {
            let count = out
                .iter()
                .rev()
                .take(3)
                .take_while(|t: &&Tok| {
                    t.plain_word().is_some_and(|word| {
                        !command_words.contains(&word) && word.chars().all(char::is_alphanumeric)
                    })
                })
                .count();
            let start = out.len() - count;
            if start < out.len() {
                let words: Vec<_> = out
                    .drain(start..)
                    // The boundary scan guarantees every drained token is a plain word.
                    .map(|t| t.plain_word().unwrap())
                    .collect();
                let separator = match case {
                    "snake" => "_",
                    "kebab" => "-",
                    _ => "",
                };
                let identifier = words
                    .iter()
                    .enumerate()
                    .map(|(index, word)| {
                        if case == "pascal" || (case == "camel" && index > 0) {
                            let mut chars = word.chars();
                            chars
                                .next()
                                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                                .unwrap_or_default()
                        } else {
                            word.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(separator);
                out.push(Tok::exact(identifier));
                i += consumed;
                continue;
            }
        }
        match word.as_str() {
            "punktum"
                if i > 0
                    && tokens
                        .get(i + 1)
                        .and_then(Tok::plain_word)
                        .is_some_and(|w| {
                            w.chars().count() <= 4
                                && w.chars().all(|c| c.is_ascii_alphanumeric())
                                && !command_words.contains(&w)
                        }) =>
            {
                out.push(Tok::sym(".", false, false));
            }
            "bindestreg" => {
                let double =
                    tokens.get(i + 1).and_then(Tok::plain_word).as_deref() == Some("bindestreg");
                let next = i + 1 + usize::from(double);
                if tokens.get(next).and_then(Tok::plain_word).is_some_and(|w| {
                    w.chars().all(|c| c.is_ascii_alphanumeric()) && !command_words.contains(&w)
                }) {
                    out.push(Tok::sym(if double { "--" } else { "-" }, true, false));
                    if let Tok::Word { text, .. } = &tokens[next] {
                        out.push(Tok::exact(text.clone()));
                    }
                    i = next + 1;
                    continue;
                }
                out.push(tokens[i].clone());
            }
            _ => out.push(tokens[i].clone()),
        }
        i += 1;
    }
    out
}
