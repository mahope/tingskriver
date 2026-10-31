//! E-mail and web addresses from speech: "anna snabel-a eksempel punktum d k"
//! → `anna@eksempel.dk`, "w w w punktum mahoje punktum dk" → `www.mahoje.dk`.
//!
//! Only kicks in when the words form an address: an "@" with a domain
//! after it, "www" followed by a domain, or a single name followed by
//! "punktum" and a well known top-level domain ("mahoje punktum dk").
//! Addresses come out in lower case without spaces. "punktum" in ordinary
//! text is left for the spoken punctuation pass.

use super::token::{Tok, WordKind};

/// Tokens an address can span ("a n n a punktum h a n s e n snabel-a ...").
const MAX_PIECES: usize = 40;

/// Top-level domains accepted after a bare name ("mahoje punktum dk").
/// Danish words such as "de", "se", "nu" and "is" are left out; spelled as
/// letters ("d e") they still work.
const BARE_TLDS: &[&str] = &[
    "dk", "com", "org", "net", "io", "eu", "ai", "dev", "app", "info", "fo", "gl", "uk", "biz",
];

/// Labels that may be followed by another top-level domain ("co.uk",
/// "com.au"). After any other complete domain a dot ends the address, so
/// "anna@eksempel.dk. Vi ses" keeps its sentence boundary.
const SECOND_LEVEL: &[&str] = &["co", "com", "org", "net", "ac", "gov", "edu"];

#[derive(Clone, Debug, PartialEq)]
enum Atom {
    Word(String),
    At,
    /// A spoken "punktum" or a dot inside a word ("eksempel.dk").
    Dot,
    /// A full stop the speech model wrote on its own ("eksempel. Dk"): a
    /// dot inside an e-mail or www address, but a sentence end after a
    /// bare name ("Den er god. Com").
    Period,
    Dash,
    Underscore,
    Slash,
    /// Something that ends any address (a comma, a line break, a snippet).
    Barrier,
}

fn word_atom(word: &str) -> Vec<Atom> {
    match word {
        "snabel-a" | "snabela" => return vec![Atom::At],
        "punktum" | "prik" | "dot" => return vec![Atom::Dot],
        "bindestreg" => return vec![Atom::Dash],
        "understreg" | "underscore" => return vec![Atom::Underscore],
        "skråstreg" => return vec![Atom::Slash],
        _ => {}
    }
    // "eksempel.dk" and "anna@eksempel.dk" from the speech model.
    let mut atoms = Vec::new();
    let mut current = String::new();
    for c in word.chars() {
        let sep = match c {
            '.' => Some(Atom::Dot),
            '@' => Some(Atom::At),
            '/' => Some(Atom::Slash),
            _ => None,
        };
        match sep {
            Some(atom) => {
                if !current.is_empty() {
                    atoms.push(Atom::Word(std::mem::take(&mut current)));
                }
                atoms.push(atom);
            }
            None => current.push(c),
        }
    }
    if !current.is_empty() {
        atoms.push(Atom::Word(current));
    }
    atoms
}

/// One address piece and the token it came from.
struct Piece {
    atom: Atom,
    /// Index of the (first) token in the token list.
    tok: usize,
    /// Tokens covered when this piece ends the address (2 for "snabel a").
    toks: usize,
    /// First and last piece of its token: an address must not start or end
    /// inside a word.
    first: bool,
    last: bool,
}

/// Address pieces for the whole token list. Tokens that can't be part of
/// an address become [`Atom::Barrier`].
fn pieces(tokens: &[Tok]) -> Vec<Piece> {
    let mut out = Vec::with_capacity(tokens.len());
    let piece = |atom, tok, toks| Piece {
        atom,
        tok,
        toks,
        first: true,
        last: true,
    };
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Tok::Punct('.') => out.push(piece(Atom::Period, i, 1)),
            Tok::Word { text, kind } if *kind != WordKind::Exact || is_address_like(text) => {
                let lower = text.to_lowercase();
                // "snabel a" as two words.
                if lower == "snabel"
                    && tokens.get(i + 1).and_then(Tok::plain_word).as_deref() == Some("a")
                {
                    out.push(piece(Atom::At, i, 2));
                    i += 2;
                    continue;
                }
                if !lower
                    .chars()
                    .all(|c| c.is_alphanumeric() || "-.@/_".contains(c))
                {
                    out.push(piece(Atom::Barrier, i, 1));
                    i += 1;
                    continue;
                }
                let atoms = word_atom(&lower);
                let n = atoms.len();
                for (k, atom) in atoms.into_iter().enumerate() {
                    out.push(Piece {
                        atom,
                        tok: i,
                        toks: 1,
                        first: k == 0,
                        last: k + 1 == n,
                    });
                }
            }
            _ => out.push(piece(Atom::Barrier, i, 1)),
        }
        i += 1;
    }
    out
}

fn is_address_like(text: &str) -> bool {
    text.contains('.') || text.contains('@')
}

fn is_single_letter(atom: &Atom) -> bool {
    matches!(atom, Atom::Word(w) if w.chars().count() == 1 && w.chars().all(char::is_alphabetic))
}

fn is_tld(word: &str, bare: bool) -> bool {
    let letters =
        (2..=6).contains(&word.chars().count()) && word.chars().all(|c| c.is_ascii_alphabetic());
    letters && (!bare || BARE_TLDS.contains(&word))
}

/// Join a label from consecutive words ("mit firma" → "mitfirma",
/// "d k" → "dk") and connectors ("anna bindestreg marie" → "anna-marie").
fn label(atoms: &[Atom]) -> String {
    atoms
        .iter()
        .map(|a| match a {
            Atom::Word(w) => w.as_str(),
            Atom::Dot | Atom::Period => ".",
            Atom::At => "@",
            Atom::Dash => "-",
            Atom::Underscore => "_",
            Atom::Slash => "/",
            Atom::Barrier => "",
        })
        .collect()
}

/// Length of a domain starting at `atoms[0]`: labels separated by dots,
/// ending in a top-level domain. Returns (atoms used, number of dots).
fn domain(atoms: &[Atom], bare: bool) -> Option<usize> {
    let mut best = None;
    // The top-level domain that ends `best`.
    let mut best_tld = String::new();
    let mut i = 0;
    let mut in_label = false;
    let mut label_start = 0;
    let mut dots = 0;
    while i < atoms.len() && i < 24 {
        match &atoms[i] {
            Atom::Word(_) => {
                if !in_label {
                    label_start = i;
                    in_label = true;
                }
                // A bare domain is a single name: "mahoje punktum dk".
                if bare
                    && dots == 0
                    && i > 0
                    && matches!(atoms[i - 1], Atom::Word(_))
                    && !is_single_letter(&atoms[i])
                {
                    return None;
                }
                if dots > 0 {
                    let tld = label(&atoms[label_start..=i]);
                    let spelled = atoms[label_start..=i].iter().all(is_single_letter);
                    let one_word = i == label_start;
                    if (one_word || spelled) && is_tld(&tld, bare && !spelled) {
                        best = Some(i + 1);
                        best_tld = tld;
                    }
                }
            }
            Atom::Dash | Atom::Underscore => {
                if !in_label || bare && dots > 0 {
                    break;
                }
            }
            Atom::Dot | Atom::Period => {
                if !in_label || bare && atoms[i] == Atom::Period {
                    break;
                }
                // "eksempel.dk" is complete: only "co" and friends take
                // another label ("co.uk").
                if best == Some(i) && !SECOND_LEVEL.contains(&best_tld.as_str()) {
                    break;
                }
                in_label = false;
                dots += 1;
                if bare && dots > 1 {
                    break;
                }
            }
            Atom::At | Atom::Slash | Atom::Barrier => break,
        }
        i += 1;
    }
    best
}

/// Path after a web address: "skråstreg kontakt" → "/kontakt".
fn path(atoms: &[Atom]) -> usize {
    let mut i = 0;
    while matches!(atoms.get(i), Some(Atom::Slash)) {
        let mut j = i + 1;
        while matches!(
            atoms.get(j),
            Some(Atom::Word(_) | Atom::Dash | Atom::Underscore)
        ) {
            j += 1;
        }
        if j == i + 1 {
            break;
        }
        i = j;
    }
    i
}

/// Start of the local part that ends just before `at`: one word, or words
/// joined by dots, hyphens or underscores, or spelled letters.
fn local_start(atoms: &[Atom], at: usize) -> Option<usize> {
    let mut i = at;
    loop {
        if i == 0 {
            break;
        }
        match &atoms[i - 1] {
            Atom::Word(_) => {
                i -= 1;
                // Spelled letters: "a n n a".
                if is_single_letter(&atoms[i]) {
                    while i > 0 && is_single_letter(&atoms[i - 1]) {
                        i -= 1;
                    }
                }
                match atoms.get(i.wrapping_sub(1)) {
                    Some(Atom::Dot | Atom::Dash | Atom::Underscore)
                        if i >= 2 && matches!(atoms[i - 2], Atom::Word(_)) =>
                    {
                        i -= 1;
                    }
                    _ => break,
                }
            }
            _ => break,
        }
    }
    (i < at).then_some(i)
}

/// Find an address in `pieces`: (first piece, end piece, text).
fn find(atoms: &[Atom]) -> Option<(usize, usize, String)> {
    // E-mail: local part, "@", domain.
    if let Some(at) = atoms.iter().position(|a| *a == Atom::At) {
        if let (Some(start), Some(len)) = (local_start(atoms, at), domain(&atoms[at + 1..], false))
        {
            let end = at + 1 + len;
            return Some((start, end, label(&atoms[start..end])));
        }
    }
    // Web address: "www" (or "w w w") and a domain.
    let www_len = match atoms {
        [Atom::Word(w), ..] if w == "www" => 1,
        [Atom::Word(a), Atom::Word(b), Atom::Word(c), ..] if a == "w" && b == "w" && c == "w" => 3,
        _ => 0,
    };
    if www_len > 0 && matches!(atoms.get(www_len), Some(Atom::Dot | Atom::Period)) {
        if let Some(len) = domain(&atoms[www_len + 1..], false) {
            let mut end = www_len + 1 + len;
            end += path(&atoms[end..]);
            return Some((0, end, format!("www.{}", label(&atoms[www_len + 1..end]))));
        }
    }
    // Bare domain: "mahoje punktum dk".
    if matches!(atoms.first(), Some(Atom::Word(_))) {
        if let Some(len) = domain(atoms, true) {
            let end = len + path(&atoms[len..]);
            return Some((0, end, label(&atoms[..end])));
        }
    }
    None
}

/// Turn spoken addresses in `tokens` into address words.
pub(super) fn format(tokens: Vec<Tok>) -> Vec<Tok> {
    let ps = pieces(&tokens);
    let atoms: Vec<Atom> = ps.iter().map(|p| p.atom.clone()).collect();
    // The first piece of each token.
    let mut first_piece = vec![None; tokens.len()];
    for (p, piece) in ps.iter().enumerate().rev() {
        first_piece[piece.tok] = Some(p);
    }

    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        if let Some(p) = first_piece[i].filter(|&p| atoms[p] != Atom::Barrier) {
            let limit = atoms.len().min(p + MAX_PIECES);
            let end = atoms[p..limit]
                .iter()
                .position(|a| *a == Atom::Barrier)
                .map_or(limit, |n| p + n);
            if let Some((start, stop, text)) = find(&atoms[p..end]) {
                let (first, last) = (&ps[p + start], &ps[p + stop - 1]);
                let from = first.tok;
                let to = last.tok + last.toks;
                if first.first && last.last && from == i && to - from > 1 {
                    out.push(Tok::exact(text));
                    i = to;
                    continue;
                }
                if from > i {
                    // The address starts further on; copy the words before it.
                    out.extend(tokens[i..from].iter().cloned());
                    i = from;
                    continue;
                }
            }
        }
        out.push(tokens[i].clone());
        i += 1;
    }
    out
}
