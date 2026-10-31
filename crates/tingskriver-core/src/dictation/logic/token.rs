//! Tokens shared by the layer 1 cleanup passes (cleanup, numbers, addresses).

/// How a word may be changed by later passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WordKind {
    /// Straight from the speech model: every pass may match and change it.
    Plain,
    /// Fixed spelling from the dictionary. Not matched again, but gets a
    /// capital letter at the start of a sentence.
    Fixed,
    /// Exact text (snippets, English terms, addresses, numbers, literal
    /// words): never matched again and never re-cased.
    Exact,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Tok {
    Word {
        text: String,
        kind: WordKind,
    },
    /// Sentence punctuation, glued to the word before it.
    Punct(char),
    /// Any other sign with its own spacing.
    Sym {
        text: String,
        space_before: bool,
        space_after: bool,
    },
    Break(u8),
}

impl Tok {
    pub(super) fn plain(text: &str) -> Tok {
        Tok::Word {
            text: text.to_string(),
            kind: WordKind::Plain,
        }
    }

    pub(super) fn exact(text: impl Into<String>) -> Tok {
        Tok::Word {
            text: text.into(),
            kind: WordKind::Exact,
        }
    }

    pub(super) fn fixed(text: impl Into<String>) -> Tok {
        Tok::Word {
            text: text.into(),
            kind: WordKind::Fixed,
        }
    }

    pub(super) fn sym(text: &str, space_before: bool, space_after: bool) -> Tok {
        Tok::Sym {
            text: text.to_string(),
            space_before,
            space_after,
        }
    }

    /// The lower-cased text of a word the passes may still change.
    pub(super) fn plain_word(&self) -> Option<String> {
        match self {
            Tok::Word {
                text,
                kind: WordKind::Plain,
            } => Some(text.to_lowercase()),
            _ => None,
        }
    }
}

/// The longest look-ahead a pass needs (a date with a spoken year, a phone
/// number spoken digit by digit).
pub(super) const MAX_LOOKAHEAD: usize = 16;

/// Lower-cased plain words from `start` up to the first token that is not
/// a plain word (at most [`MAX_LOOKAHEAD`]). Used by the passes that look
/// ahead over several words.
pub(super) fn plain_run(tokens: &[Tok], start: usize) -> Vec<String> {
    tokens[start..]
        .iter()
        .take(MAX_LOOKAHEAD)
        .map_while(Tok::plain_word)
        .collect()
}
