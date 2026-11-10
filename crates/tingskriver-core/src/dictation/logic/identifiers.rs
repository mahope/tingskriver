//! Pure code-style token formatting, before spoken punctuation is expanded.

use super::cleanup::Style;
use super::token::Tok;

pub(super) fn format(tokens: Vec<Tok>, style: Style) -> Vec<Tok> {
    if style != Style::Raw {
        return tokens;
    }
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
            let start = out
                .iter()
                .rposition(|t: &Tok| {
                    t.plain_word().is_none()
                        || matches!(
                            t.plain_word().as_deref(),
                            Some("komma" | "semikolon" | "kolon" | "punktum")
                        )
                })
                .map_or(0, |index| index + 1);
            if start < out.len() {
                let words: Vec<_> = out
                    .drain(start..)
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
            "punktum" if i > 0 && tokens.get(i + 1).and_then(Tok::plain_word).is_some() => {
                out.push(Tok::sym(".", false, false));
            }
            "bindestreg"
                if tokens
                    .get(i + 1)
                    .and_then(Tok::plain_word)
                    .is_some_and(|w| w.chars().count() == 1) =>
            {
                out.push(Tok::sym("-", true, false));
            }
            _ => out.push(tokens[i].clone()),
        }
        i += 1;
    }
    out
}
