//! Layer 1 cleanup of dictated text (deterministic, no LLM).
//!
//! Removes Danish filler words, turns spoken punctuation into characters,
//! fixes spacing before punctuation, capitalises sentence starts and applies
//! the user's dictionary and snippets. Runs in well under a millisecond for
//! normal dictation lengths.

/// How much the text is touched. `Raw` is used in code editors and terminals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Fillers removed, sentence starts capitalised.
    Normal,
    /// Only "øh"-type fillers are removed and nothing is capitalised.
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    pub from: String,
    pub to: String,
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
    "pga", "vedr", "jf", "ift", "hhv", "tlf", "etc", "mr", "hr", "fr",
];

const TRAILING_PUNCT: &[char] = &['.', ',', '?', '!', ':', ';'];

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Word { text: String, protected: bool },
    Punct(char),
    Break(u8),
}

#[derive(Clone, Debug)]
enum Action {
    Punct(char),
    Break(u8),
    /// Fixed text. `drop_following_punct` is set for snippets, whose trigger
    /// usually gets a period from the speech model.
    Text {
        text: String,
        drop_following_punct: bool,
    },
}

struct Matcher {
    words: Vec<String>,
    action: Action,
}

/// Clean dictated text according to `style` and the user's `rules`.
pub fn clean(input: &str, style: Style, rules: &Rules) -> String {
    let tokens = tokenize(input);
    let tokens = apply_phrases(tokens, &build_matchers(rules));
    let tokens = remove_fillers(tokens, style);
    let tokens = if style == Style::Normal {
        capitalize(tokens)
    } else {
        tokens
    };
    render(&tokens)
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

fn build_matchers(rules: &Rules) -> Vec<Matcher> {
    let mut matchers = Vec::new();

    // User rules come first so they can override built-in commands.
    for s in &rules.snippets {
        let words = phrase_words(&s.trigger);
        if !words.is_empty() && !s.text.is_empty() {
            matchers.push(Matcher {
                words,
                action: Action::Text {
                    text: s.text.clone(),
                    drop_following_punct: true,
                },
            });
        }
    }
    for r in &rules.replacements {
        let words = phrase_words(&r.from);
        if !words.is_empty() && !r.to.trim().is_empty() {
            matchers.push(Matcher {
                words,
                action: Action::Text {
                    text: r.to.trim().to_string(),
                    drop_following_punct: false,
                },
            });
        }
    }

    let commands: &[(&str, Action)] = &[
        ("punktum", Action::Punct('.')),
        ("komma", Action::Punct(',')),
        ("spørgsmålstegn", Action::Punct('?')),
        ("udråbstegn", Action::Punct('!')),
        ("kolon", Action::Punct(':')),
        ("semikolon", Action::Punct(';')),
        ("ny linje", Action::Break(1)),
        ("ny linie", Action::Break(1)),
        ("nyt afsnit", Action::Break(2)),
    ];
    for (phrase, action) in commands {
        matchers.push(Matcher {
            words: phrase_words(phrase),
            action: action.clone(),
        });
    }

    // Longest phrase wins; the sort is stable so user rules win ties.
    matchers.sort_by(|a, b| b.words.len().cmp(&a.words.len()));
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
                tokens.push(Tok::Word {
                    text: core.to_string(),
                    protected: false,
                });
            }
            for c in rest[core.len()..].chars() {
                tokens.push(Tok::Punct(c));
            }
        }
    }
    tokens
}

fn match_at<'m>(tokens: &[Tok], start: usize, matchers: &'m [Matcher]) -> Option<&'m Matcher> {
    matchers.iter().find(|m| {
        if start + m.words.len() > tokens.len() {
            return false;
        }
        m.words.iter().enumerate().all(|(offset, word)| match &tokens[start + offset] {
            Tok::Word {
                text,
                protected: false,
            } => normalize(text) == *word,
            _ => false,
        })
    })
}

fn apply_phrases(tokens: Vec<Tok>, matchers: &[Matcher]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let matched = match &tokens[i] {
            Tok::Word { .. } => match_at(&tokens, i, matchers),
            _ => None,
        };
        let Some(m) = matched else {
            match &tokens[i] {
                Tok::Punct(c) => push_punct(&mut out, *c),
                Tok::Break(n) => push_break(&mut out, *n),
                word => out.push(word.clone()),
            }
            i += 1;
            continue;
        };

        i += m.words.len();
        let drop_following = match &m.action {
            Action::Punct(c) => {
                push_punct(&mut out, *c);
                true
            }
            Action::Break(n) => {
                // A comma the speech model put before "ny linje" is noise.
                while matches!(out.last(), Some(Tok::Punct(',' | ';'))) {
                    out.pop();
                }
                push_break(&mut out, *n);
                true
            }
            Action::Text {
                text,
                drop_following_punct,
            } => {
                out.push(Tok::Word {
                    text: text.clone(),
                    protected: true,
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
        Some(Tok::Word { .. }) => out.push(Tok::Punct(c)),
    }
}

fn push_break(out: &mut Vec<Tok>, n: u8) {
    if let Some(Tok::Break(prev)) = out.last_mut() {
        *prev = (*prev + n).min(2);
    } else {
        out.push(Tok::Break(n.min(2)));
    }
}

fn is_sentence_start(out: &[Tok]) -> bool {
    match out.last() {
        None | Some(Tok::Break(_)) => true,
        Some(Tok::Punct(c)) => matches!(c, '.' | '?' | '!') && !ends_with_abbreviation(out),
        Some(Tok::Word { .. }) => false,
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
        let is_filler = match tok {
            Tok::Word {
                text,
                protected: false,
            } => {
                let w = normalize(text);
                HESITATIONS.contains(&w.as_str())
                    || (style == Style::Normal
                        && SENTENCE_FILLERS.contains(&w.as_str())
                        && is_sentence_start(&out))
            }
            _ => false,
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
            word => out.push(word.clone()),
        }
        i += 1;
    }
    out
}

fn capitalize(tokens: Vec<Tok>) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    for tok in tokens {
        let at_start = is_sentence_start(&out);
        match tok {
            Tok::Word {
                text,
                protected: false,
            } if at_start => out.push(Tok::Word {
                text: upper_first(&text),
                protected: false,
            }),
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
                out.push(*c);
                need_space = true;
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
    out.trim_end_matches(' ').trim_start_matches(' ').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normal(input: &str) -> String {
        clean(input, Style::Normal, &Rules::default())
    }

    fn raw(input: &str) -> String {
        clean(input, Style::Raw, &Rules::default())
    }

    fn rules() -> Rules {
        Rules {
            replacements: vec![
                Replacement {
                    from: "Mahoje".into(),
                    to: "Mahoje".into(),
                },
                Replacement {
                    from: "maho je".into(),
                    to: "Mahoje".into(),
                },
                Replacement {
                    from: "syv ai".into(),
                    to: "Syv.ai".into(),
                },
            ],
            snippets: vec![
                Snippet {
                    trigger: "min signatur".into(),
                    text: "Mads\nMahope".into(),
                },
                Snippet {
                    trigger: "mit cvr".into(),
                    text: "39413892".into(),
                },
            ],
        }
    }

    #[test]
    fn empty_and_whitespace() {
        assert_eq!(normal(""), "");
        assert_eq!(normal("   "), "");
    }

    #[test]
    fn removes_leading_fillers() {
        assert_eq!(normal("øh jeg kommer i morgen"), "Jeg kommer i morgen");
        assert_eq!(normal("Øhm, jeg kommer i morgen."), "Jeg kommer i morgen.");
        assert_eq!(normal("Altså, det er fint."), "Det er fint.");
        assert_eq!(normal("øh altså øhh det er fint"), "Det er fint");
    }

    #[test]
    fn removes_fillers_between_sentences() {
        assert_eq!(
            normal("Det er godt. Øh, vi ses i morgen."),
            "Det er godt. Vi ses i morgen."
        );
        assert_eq!(
            normal("Det er godt. Altså, vi ses i morgen."),
            "Det er godt. Vi ses i morgen."
        );
        assert_eq!(normal("jeg tror øh at det går"), "Jeg tror at det går");
    }

    #[test]
    fn keeps_altsaa_mid_sentence() {
        assert_eq!(normal("han er altså sød"), "Han er altså sød");
    }

    #[test]
    fn filler_before_sentence_end_keeps_period() {
        assert_eq!(normal("det var det øh."), "Det var det.");
    }

    #[test]
    fn does_not_touch_fillers_inside_words() {
        assert_eq!(normal("køhm og søhest"), "Køhm og søhest");
        assert_eq!(normal("altsåi"), "Altsåi");
    }

    #[test]
    fn spoken_punctuation() {
        assert_eq!(
            normal("hej Mads komma hvordan går det spørgsmålstegn"),
            "Hej Mads, hvordan går det?"
        );
        assert_eq!(normal("det er fint punktum tak"), "Det er fint. Tak");
        assert_eq!(normal("husk udråbstegn"), "Husk!");
        assert_eq!(
            normal("tre ting kolon mælk og brød"),
            "Tre ting: mælk og brød"
        );
        assert_eq!(normal("a semikolon b"), "A; b");
    }

    #[test]
    fn spoken_punctuation_with_model_punctuation() {
        assert_eq!(
            normal("Hej Mads, komma, hvordan går det, spørgsmålstegn."),
            "Hej Mads, hvordan går det?"
        );
        assert_eq!(normal("Det er fint. Punktum."), "Det er fint.");
        assert_eq!(normal("Komma, hej"), "Hej");
    }

    #[test]
    fn new_line_and_paragraph() {
        assert_eq!(normal("hej ny linje hvordan går det"), "Hej\nHvordan går det");
        assert_eq!(normal("Hej, ny linje, tak."), "Hej\nTak.");
        assert_eq!(normal("Hej. Nyt afsnit. Tak."), "Hej.\n\nTak.");
        assert_eq!(normal("hej ny linje ny linje tak"), "Hej\n\nTak");
        assert_eq!(normal("hej ny linie tak"), "Hej\nTak");
    }

    #[test]
    fn fixes_space_before_punctuation() {
        assert_eq!(normal("hej , du . hvad så ?"), "Hej, du. Hvad så?");
        assert_eq!(normal("hej  med   dig"), "Hej med dig");
    }

    #[test]
    fn capitalises_sentence_starts() {
        assert_eq!(normal("hej. hvad så? godt! ja"), "Hej. Hvad så? Godt! Ja");
        assert_eq!(normal("ældre æbler"), "Ældre æbler");
        assert_eq!(normal("øl er godt"), "Øl er godt");
    }

    #[test]
    fn abbreviations_do_not_end_sentences() {
        assert_eq!(normal("vi ses kl. tre"), "Vi ses kl. tre");
        assert_eq!(normal("frugt f.eks. æbler"), "Frugt f.eks. æbler");
        assert_eq!(normal("ca. ti"), "Ca. ti");
    }

    #[test]
    fn keeps_numbers_and_inner_punctuation() {
        assert_eq!(normal("det koster 3.5 kroner"), "Det koster 3.5 kroner");
        assert_eq!(normal("skriv til mads@mahoje.dk"), "Skriv til mads@mahoje.dk");
    }

    #[test]
    fn dictionary_replacements() {
        let r = rules();
        assert_eq!(
            clean("jeg arbejder hos maho je", Style::Normal, &r),
            "Jeg arbejder hos Mahoje"
        );
        assert_eq!(
            clean("mahoje bruger syv ai.", Style::Normal, &r),
            "Mahoje bruger Syv.ai."
        );
        assert_eq!(
            clean("MAHOJE", Style::Normal, &r),
            "Mahoje"
        );
    }

    #[test]
    fn snippets() {
        let r = rules();
        assert_eq!(clean("Min signatur.", Style::Normal, &r), "Mads\nMahope");
        assert_eq!(
            clean("vores cvr er mit cvr punktum", Style::Normal, &r),
            "Vores cvr er 39413892."
        );
    }

    #[test]
    fn raw_style() {
        assert_eq!(raw("øh git status"), "git status");
        assert_eq!(raw("altså npm install"), "altså npm install");
        assert_eq!(raw("cargo test komma tak"), "cargo test, tak");
        assert_eq!(raw("ls ny linje pwd"), "ls\npwd");
    }

    #[test]
    fn model_newlines_are_kept() {
        assert_eq!(normal("første linje\nanden linje"), "Første linje\nAnden linje");
        assert_eq!(normal("a\n\n\n\nb"), "A\n\nB");
    }

    #[test]
    fn only_fillers_gives_empty_text() {
        assert_eq!(normal("øh øhm."), "");
        assert_eq!(normal("Øh."), "");
    }

    #[test]
    fn prompt_words_are_unique_targets() {
        assert_eq!(prompt_words(&rules()), vec!["Mahoje", "Syv.ai"]);
    }

    #[test]
    fn style_parsing() {
        assert_eq!(Style::parse("raw"), Style::Raw);
        assert_eq!(Style::parse("rå"), Style::Raw);
        assert_eq!(Style::parse("normal"), Style::Normal);
        assert_eq!(Style::parse("whatever"), Style::Normal);
    }

    #[test]
    fn long_text_is_fast() {
        let sentence = "øh jeg tror altså at det komma som vi sagde punktum ";
        let input = sentence.repeat(200);
        let start = std::time::Instant::now();
        let out = clean(&input, Style::Normal, &rules());
        assert!(!out.is_empty());
        assert!(start.elapsed().as_millis() < 50, "took {:?}", start.elapsed());
    }
}
