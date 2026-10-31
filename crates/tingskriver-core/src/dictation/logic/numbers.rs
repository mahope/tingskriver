//! Danish number formatting in layer 1: spoken numbers, dates, times,
//! weeks, amounts, percentages, phone and CVR numbers.
//!
//! The conventions follow common Danish usage (Sprogrådet's recommendations
//! as far as they are known here; not checked against a source):
//! - numbers up to twelve stay words in running text ("to æbler"), larger
//!   ones become digits ("tretten" → 13);
//! - with a unit or in a fixed context digits are always used: "12 %",
//!   "5 kr.", "kl. 8", "uge 2", "3. november";
//! - thousands are grouped with a period and decimals use a comma:
//!   "1.350", "12,5 %"; years are not grouped ("i 2026");
//! - times use a period: "kl. 14.30"; phone numbers are written in pairs.
//!
//! Only plain words from the speech model are touched. Words that already
//! contain letters and digits, slashes or dots (file names, versions, URLs)
//! are never changed, and the pass is off in the raw style (code editors).

use super::token::{plain_run, Tok};

const UNITS: &[(&str, u64)] = &[
    ("nul", 0),
    ("en", 1),
    ("et", 1),
    ("én", 1),
    ("ét", 1),
    ("to", 2),
    ("tre", 3),
    ("fire", 4),
    ("fem", 5),
    ("seks", 6),
    ("syv", 7),
    ("otte", 8),
    ("ni", 9),
];

const TEENS: &[(&str, u64)] = &[
    ("ti", 10),
    ("elleve", 11),
    ("tolv", 12),
    ("tretten", 13),
    ("fjorten", 14),
    ("femten", 15),
    ("seksten", 16),
    ("sytten", 17),
    ("atten", 18),
    ("nitten", 19),
];

const TENS: &[(&str, u64)] = &[
    ("tyve", 20),
    ("tredive", 30),
    ("tredve", 30),
    ("fyrre", 40),
    ("fyrretyve", 40),
    ("halvtreds", 50),
    ("halvtredsindstyve", 50),
    ("tres", 60),
    ("tresindstyve", 60),
    ("halvfjerds", 70),
    ("halvfjerdsindstyve", 70),
    ("firs", 80),
    ("firsindstyve", 80),
    ("halvfems", 90),
    ("halvfemsindstyve", 90),
];

const ORDINALS: &[(&str, u64)] = &[
    ("første", 1),
    ("anden", 2),
    ("andet", 2),
    ("tredje", 3),
    ("fjerde", 4),
    ("femte", 5),
    ("sjette", 6),
    ("syvende", 7),
    ("ottende", 8),
    ("niende", 9),
    ("tiende", 10),
    ("ellevte", 11),
    ("tolvte", 12),
    ("trettende", 13),
    ("fjortende", 14),
    ("femtende", 15),
    ("sekstende", 16),
    ("syttende", 17),
    ("attende", 18),
    ("nittende", 19),
    ("tyvende", 20),
    ("tredivte", 30),
    ("tredevte", 30),
];

const MONTHS: &[&str] = &[
    "januar",
    "februar",
    "marts",
    "april",
    "maj",
    "juni",
    "juli",
    "august",
    "september",
    "oktober",
    "november",
    "december",
];

/// Words before a number that make it a year ("i 2026", "siden 1980").
const YEAR_CUES: &[&str] = &[
    "i", "år", "året", "siden", "fra", "indtil", "inden", "omkring", "årstal",
];

const PHONE_CUES: &[&str] = &[
    "telefon",
    "telefonnummer",
    "telefonnummeret",
    "tlf",
    "tlf.",
    "mobil",
    "mobilnummer",
    "mobilnummeret",
    "mobilnr",
];
const CVR_CUES: &[&str] = &[
    "cvr",
    "cvr-nummer",
    "cvr-nr",
    "cvr-nr.",
    "cvrnummer",
    "cvr-nummeret",
];
/// Words allowed between a cue and the digits ("telefon nummer er ...").
const CUE_FILLERS: &[&str] = &["nummer", "nummeret", "nr", "nr.", "er", "på", "-nummer"];

fn lookup(table: &[(&str, u64)], word: &str) -> Option<u64> {
    table.iter().find(|(w, _)| *w == word).map(|(_, v)| *v)
}

fn is_article(word: &str) -> bool {
    matches!(word, "en" | "et" | "én" | "ét")
}

fn unit(word: &str) -> Option<u64> {
    lookup(UNITS, word)
}

fn tens(word: &str) -> Option<u64> {
    lookup(TENS, word)
}

/// "seksogtyve" → 26, "enogtredive" → 31.
fn compound(word: &str) -> Option<u64> {
    let (u, t) = word.split_once("og")?;
    let u = match u {
        "en" | "én" => 1,
        other => unit(other).filter(|v| (2..=9).contains(v))?,
    };
    Some(u + tens(t)?)
}

/// "seksogtyvende" → 26, "enogtredivte" → 31.
fn ordinal(word: &str) -> Option<u64> {
    if let Some(v) = lookup(ORDINALS, word) {
        return Some(v);
    }
    let (u, t) = word.split_once("og")?;
    let u = match u {
        "en" | "én" => 1,
        other => unit(other).filter(|v| (2..=9).contains(v))?,
    };
    let t = lookup(ORDINALS, t).filter(|v| *v == 20 || *v == 30)?;
    Some(u + t)
}

fn is_digits(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| c.is_ascii_digit())
}

fn scale(word: &str) -> Option<u64> {
    match word {
        "hundrede" | "hundred" => Some(100),
        "tusind" | "tusinde" => Some(1_000),
        "million" | "millioner" => Some(1_000_000),
        "milliard" | "milliarder" => Some(1_000_000_000),
        _ => None,
    }
}

/// A parsed number: value, words used, and whether it was a single word
/// that normally stays a word in running text ("to", "tolv", "hundrede").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Num {
    value: u64,
    len: usize,
    small_word: bool,
    /// Built as "<n> hundrede" with n ≥ 10 ("nitten hundrede og firs").
    hundreds_form: bool,
}

/// 0-99 (also digit tokens up to three digits as multipliers).
fn below_100(w: &[String], i: usize) -> Option<Num> {
    let word = w.get(i)?.as_str();
    let num = |value, len, small_word| Num {
        value,
        len,
        small_word,
        hundreds_form: false,
    };
    if is_digits(word) && word.len() <= 3 {
        return word.parse().ok().map(|v| num(v, 1, false));
    }
    if let Some(v) = compound(word) {
        return Some(num(v, 1, false));
    }
    if let Some(v) = tens(word).or_else(|| lookup(TEENS, word)) {
        return Some(num(v, 1, v <= 12));
    }
    let u = unit(word)?;
    if let (Some("og"), Some(t)) = (
        w.get(i + 1).map(String::as_str),
        w.get(i + 2).and_then(|t| tens(t)),
    ) {
        if u >= 1 {
            return Some(num(u + t, 3, false));
        }
    }
    if is_article(word) {
        return None;
    }
    Some(num(u, 1, true))
}

/// 0-9999 with "hundrede": "to hundrede og halvtreds", "tretten hundrede".
fn below_1000(w: &[String], i: usize) -> Option<Num> {
    let word = w.get(i).map(String::as_str)?;
    let (hundreds, mut j, multiplier) = if scale(word) == Some(100) {
        (100, i + 1, 1)
    } else if is_article(word) && w.get(i + 1).and_then(|s| scale(s)) == Some(100) {
        (100, i + 2, 1)
    } else {
        let a = below_100(w, i)?;
        if w.get(i + a.len).and_then(|s| scale(s)) != Some(100) {
            return Some(a);
        }
        (a.value * 100, i + a.len + 1, a.value)
    };
    let mut value = hundreds;
    let rest_at = if w.get(j).map(String::as_str) == Some("og") {
        j + 1
    } else {
        j
    };
    if let Some(rest) = below_100(w, rest_at).filter(|r| r.value < 100) {
        value += rest.value;
        j = rest_at + rest.len;
    }
    Some(Num {
        value,
        len: j - i,
        small_word: j - i == 1,
        hundreds_form: multiplier >= 10,
    })
}

/// Any cardinal up to the milliards.
fn cardinal(w: &[String], i: usize) -> Option<Num> {
    scaled(w, i, 0)
}

const BIG: &[u64] = &[1_000_000_000, 1_000_000, 1_000];

fn scaled(w: &[String], i: usize, level: usize) -> Option<Num> {
    let Some(&factor) = BIG.get(level) else {
        return below_1000(w, i);
    };
    let is_factor = |k: usize| w.get(k).and_then(|s| scale(s)) == Some(factor);
    let (head, j) = if is_factor(i) {
        (1, i + 1)
    } else if w.get(i).is_some_and(|s| is_article(s)) && is_factor(i + 1) {
        (1, i + 2)
    } else {
        let m = scaled(w, i, level + 1)?;
        if !is_factor(i + m.len) {
            return Some(m);
        }
        (m.value, i + m.len + 1)
    };
    let mut value = head * factor;
    let mut end = j;
    let rest_at = if w.get(j).map(String::as_str) == Some("og") {
        j + 1
    } else {
        j
    };
    if let Some(rest) = scaled(w, rest_at, level + 1).filter(|r| r.value < factor) {
        value += rest.value;
        end = rest_at + rest.len;
    }
    Some(Num {
        value,
        len: end - i,
        small_word: end - i == 1,
        hundreds_form: false,
    })
}

/// "1350" → "1.350".
pub fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    if digits.len() < 4 {
        return digits;
    }
    let mut out = String::new();
    for (n, c) in digits.chars().enumerate() {
        if n > 0 && (digits.len() - n) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// What a unit word becomes after a number.
fn unit_tokens(word: &str) -> Option<Vec<Tok>> {
    Some(match word {
        "procent" | "pct" | "pct." | "%" => vec![Tok::sym("%", true, true)],
        "kroner" | "krone" | "kr" | "kr." | "kroners" => vec![Tok::exact("kr.")],
        "euro" | "dollar" | "dollars" | "grader" | "grad" | "kilometer" | "km" | "meter"
        | "kilo" | "kg" | "gram" | "liter" | "mio." | "mia." => vec![Tok::plain(word)],
        "mio" | "mia" => vec![Tok::exact(format!("{word}."))],
        _ => return None,
    })
}

/// Digits after "komma": "fem" → "5", "nul fem" → "05", "femogtyve" → "25".
fn fraction(w: &[String], i: usize) -> Option<(String, usize)> {
    let mut digits = String::new();
    let mut j = i;
    while let Some(word) = w.get(j) {
        match unit(word).filter(|_| !is_article(word)) {
            Some(d) => digits.push_str(&d.to_string()),
            None => break,
        }
        j += 1;
    }
    if !digits.is_empty() {
        return Some((digits, j - i));
    }
    let word = w.get(i)?;
    if is_digits(word) {
        return Some((word.clone(), 1));
    }
    let n = below_100(w, i)?;
    Some((n.value.to_string(), n.len))
}

/// A digit token from the speech model in English or Danish format,
/// returned as Danish text: "3.5" → "3,5", "5000" → "5.000", "1,350,000" → "1.350.000".
fn danish_digits(word: &str, money: bool) -> Option<String> {
    if is_digits(word) {
        let v: u64 = word.parse().ok()?;
        return Some(if money {
            group_thousands(v)
        } else {
            word.to_string()
        });
    }
    let chars_ok = word
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',');
    if !chars_ok
        || !word.starts_with(|c: char| c.is_ascii_digit())
        || !word.ends_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    let dots = word.matches('.').count();
    let commas = word.matches(',').count();
    let groups_of_three = |sep: char| word.split(sep).skip(1).all(|g| g.len() == 3);
    match (dots, commas) {
        // Danish thousands ("5.000", "1.350.000") or English decimal ("3.5").
        (_, 0) if groups_of_three('.') => Some(word.to_string()),
        (1, 0) => Some(word.replace('.', ",")),
        // English thousands ("1,350,000"); a single ",350" only for money.
        (0, n) if groups_of_three(',') && (n > 1 || money) => Some(word.replace(',', ".")),
        // Danish decimal ("12,5").
        (0, 1) => Some(word.to_string()),
        (0, _) | (_, 0) => None,
        // Both: the last separator is the decimal one.
        _ => {
            let (last_dot, last_comma) = (word.rfind('.'), word.rfind(','));
            if last_comma > last_dot {
                // Danish "1.350,50".
                Some(word.to_string())
            } else {
                // English "1,350.50".
                Some(
                    word.replace(',', "\u{0}")
                        .replace('.', ",")
                        .replace('\u{0}', "."),
                )
            }
        }
    }
}

/// The formatted replacement for the words at `w[i..]`, if any.
struct Rewrite {
    /// Leading words kept as they were ("CVR-nummer", "uge").
    keep: usize,
    tokens: Vec<Tok>,
    /// Plain words consumed after the kept ones.
    len: usize,
}

fn rewrite(w: &[String], i: usize, prev: Option<&str>) -> Option<Rewrite> {
    date(w, i)
        .or_else(|| time(w, i))
        .or_else(|| week(w, i))
        .or_else(|| phone_or_cvr(w, i))
        .or_else(|| amount(w, i))
        .or_else(|| loose_digits(w, i))
        .or_else(|| plain_number(w, i, prev))
}

/// "tredje november [to tusind og seksogtyve]" → "3. november [2026]".
fn date(w: &[String], i: usize) -> Option<Rewrite> {
    let day = ordinal(&w[i]).filter(|d| (1..=31).contains(d))?;
    let month = w.get(i + 1).filter(|m| MONTHS.contains(&m.as_str()))?;
    let mut tokens = vec![Tok::exact(format!("{day}.")), Tok::plain(month)];
    let mut len = 2;
    if let Some((year, n)) = year(w, i + 2) {
        tokens.push(Tok::exact(year.to_string()));
        len += n;
    }
    Some(Rewrite {
        keep: 0,
        tokens,
        len,
    })
}

/// "to tusind og seksogtyve", "nitten hundrede og firs" or "tyve seksogtyve".
fn year(w: &[String], i: usize) -> Option<(u64, usize)> {
    if let Some(n) = cardinal(w, i).filter(|n| (1000..=2199).contains(&n.value)) {
        return Some((n.value, n.len));
    }
    let century = below_100(w, i).filter(|c| (19..=21).contains(&c.value) && c.len == 1)?;
    let rest = below_100(w, i + 1).filter(|r| r.value >= 10 && r.value < 100)?;
    Some((century.value * 100 + rest.value, 1 + rest.len))
}

fn hour_and_minutes(word: &str) -> Option<String> {
    let (h, m) = word.split_once([':', '.'])?;
    let hour: u64 = h.parse().ok().filter(|_| is_digits(h))?;
    let minutes: u64 = m.parse().ok().filter(|_| is_digits(m) && m.len() == 2)?;
    (hour <= 24 && minutes < 60).then(|| format!("{hour}.{m}"))
}

/// "klokken fjorten tredive" → "kl. 14.30", "klokken otte" → "kl. 8".
fn time(w: &[String], i: usize) -> Option<Rewrite> {
    if !matches!(w[i].as_str(), "klokken" | "klokka" | "kl" | "kl.") {
        return None;
    }
    let at = i + 1;
    let word = w.get(at)?;
    if let Some(text) = hour_and_minutes(word) {
        return Some(Rewrite {
            keep: 0,
            tokens: vec![Tok::fixed("kl."), Tok::exact(text)],
            len: 2,
        });
    }
    let hour = if word == "et" || word == "én" || word == "ét" {
        Num {
            value: 1,
            len: 1,
            small_word: true,
            hundreds_form: false,
        }
    } else {
        below_100(w, at)?
    };
    if hour.value > 24 {
        return None;
    }
    let mut j = at + hour.len;
    // "klokken ti over fem", "klokken fem minutter i" are left alone.
    if w.get(j).is_some_and(|n| {
        matches!(
            n.as_str(),
            "over" | "i" | "minutter" | "minut" | "halv" | "kvart"
        )
    }) {
        return None;
    }
    let mut text = hour.value.to_string();
    let minutes = if w.get(j).map(String::as_str) == Some("nul") {
        w.get(j + 1)
            .and_then(|m| unit(m))
            .filter(|m| *m <= 9)
            .map(|m| (m, 2))
    } else {
        below_100(w, j)
            .filter(|m| m.value < 60 && !(m.small_word && m.value < 10))
            .map(|m| (m.value, m.len))
    };
    if let Some((m, n)) = minutes {
        text = format!("{text}.{m:02}");
        j += n;
    }
    Some(Rewrite {
        keep: 0,
        tokens: vec![Tok::fixed("kl."), Tok::exact(text)],
        len: j - i,
    })
}

/// "uge fireogfyrre" → "uge 44".
fn week(w: &[String], i: usize) -> Option<Rewrite> {
    if w[i] != "uge" {
        return None;
    }
    let n = below_100(w, i + 1).filter(|n| (1..=53).contains(&n.value))?;
    Some(Rewrite {
        keep: 1,
        tokens: vec![Tok::exact(n.value.to_string())],
        len: n.len,
    })
}

/// Digits spoken one or two at a time: "tolv fireogtredive" → "1234".
fn digit_groups(w: &[String], i: usize, max: usize) -> (String, usize) {
    let mut digits = String::new();
    let mut j = i;
    while digits.len() < max {
        let Some(word) = w.get(j) else { break };
        if is_digits(word) {
            digits.push_str(word);
            j += 1;
            continue;
        }
        // Pairs first ("fem og fyrre"), then single digits; "en" is a digit
        // here: "tre ni fire en ...".
        if let Some(n) = below_100(w, j).filter(|n| n.value >= 10) {
            digits.push_str(&n.value.to_string());
            j += n.len;
            continue;
        }
        match unit(word) {
            Some(d) => {
                digits.push_str(&d.to_string());
                j += 1;
            }
            None => break,
        }
    }
    (digits, j - i)
}

fn pairs(digits: &str) -> String {
    let chars: Vec<char> = digits.chars().collect();
    chars
        .chunks(2)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// "telefon tolv tredive fyrre halvtreds" → "telefon 12 30 40 50";
/// "cvr tre ni fire en tre otte ni to" → "cvr 39413892".
fn phone_or_cvr(w: &[String], i: usize) -> Option<Rewrite> {
    let cue = w[i].as_str();
    let is_cvr = CVR_CUES.contains(&cue);
    if !is_cvr && !PHONE_CUES.contains(&cue) {
        return None;
    }
    let mut j = i + 1;
    let mut fillers = 0;
    while w.get(j).is_some_and(|f| CUE_FILLERS.contains(&f.as_str())) {
        fillers += 1;
        j += 1;
    }
    let mut prefix = String::new();
    if !is_cvr && w.get(j).map(String::as_str) == Some("plus") {
        let (cc, n) = digit_groups(w, j + 1, 2);
        if cc.len() == 2 {
            prefix = format!("+{cc} ");
            j += 1 + n;
        }
    }
    let (digits, n) = digit_groups(w, j, 8);
    if digits.len() != 8 {
        return None;
    }
    // Already written as one number by the model and nothing to change.
    if n == 1 && prefix.is_empty() && is_cvr {
        return None;
    }
    let number = if is_cvr {
        digits
    } else {
        format!("{prefix}{}", pairs(&digits))
    };
    let keep = 1 + fillers;
    Some(Rewrite {
        keep,
        tokens: vec![Tok::exact(number)],
        len: j + n - i - keep,
    })
}

/// Eight single digits in a row become one number: "tre ni fire ..." → "39413892".
fn loose_digits(w: &[String], i: usize) -> Option<Rewrite> {
    let mut digits = String::new();
    for word in w.iter().skip(i).take(8) {
        match unit(word) {
            Some(d) => digits.push_str(&d.to_string()),
            None => return None,
        }
    }
    (digits.len() == 8).then(|| Rewrite {
        keep: 0,
        tokens: vec![Tok::exact(digits)],
        len: 8,
    })
}

/// A number followed by a unit: "tolv procent" → "12 %", "fem tusind kroner"
/// → "5.000 kr.", "tolv komma fem procent" → "12,5 %", "3.5%" → "3,5 %".
fn amount(w: &[String], i: usize) -> Option<Rewrite> {
    let word = &w[i];
    // "12%" in one token.
    if let Some(number) = word.strip_suffix('%') {
        let number = danish_digits(number, false)?;
        return Some(Rewrite {
            keep: 0,
            tokens: vec![Tok::exact(number), Tok::sym("%", true, true)],
            len: 1,
        });
    }
    let (number, mut j) = if word.starts_with(|c: char| c.is_ascii_digit()) {
        let money = w
            .get(i + 1)
            .is_some_and(|u| matches!(u.as_str(), "kroner" | "krone" | "kr" | "kr." | "euro"));
        (danish_digits(word, money)?, i + 1)
    } else {
        let n = cardinal(w, i)?;
        (group_thousands(n.value), i + n.len)
    };
    let mut number = number;
    if w.get(j).map(String::as_str) == Some("komma") && !number.contains(',') {
        if let Some((digits, n)) = fraction(w, j + 1) {
            let after = j + 1 + n;
            let unit_follows = w.get(after).is_some_and(|u| {
                unit_tokens(u).is_some() || matches!(scale(u), Some(s) if s >= 1_000_000)
            });
            if unit_follows {
                number = format!("{number},{digits}");
                j = after;
            }
        }
    }
    let unit_word = w.get(j)?;
    let mut tokens = vec![Tok::exact(number)];
    if scale(unit_word).is_some_and(|s| s >= 1_000_000) {
        // "tre komma fem millioner [kroner]"
        tokens.push(Tok::plain(unit_word));
        j += 1;
        if let Some(rest) = w.get(j).and_then(|u| unit_tokens(u)) {
            tokens.extend(rest);
            j += 1;
        }
        return Some(Rewrite {
            keep: 0,
            tokens,
            len: j - i,
        });
    }
    tokens.extend(unit_tokens(unit_word)?);
    Some(Rewrite {
        keep: 0,
        tokens,
        len: j + 1 - i,
    })
}

/// A number on its own: "tretten hundrede og halvtreds" → "1.350",
/// "i to tusind og seksogtyve" → "i 2026". Up to twelve stays a word.
fn plain_number(w: &[String], i: usize, prev: Option<&str>) -> Option<Rewrite> {
    if is_digits(&w[i]) {
        return None;
    }
    let n = cardinal(w, i)?;
    if n.small_word {
        return None;
    }
    let is_year = (1900..=2100).contains(&n.value)
        && (prev.is_some_and(|p| YEAR_CUES.contains(&p) || MONTHS.contains(&p))
            || (n.hundreds_form && n.value >= 1900));
    // "tre millioner" → "3 millioner", not "3.000.000".
    let last = &w[i + n.len - 1];
    if let Some(s) = scale(last).filter(|s| *s >= 1_000_000 && n.value % s == 0) {
        return Some(Rewrite {
            keep: 0,
            tokens: vec![Tok::exact((n.value / s).to_string()), Tok::plain(last)],
            len: n.len,
        });
    }
    let text = if is_year {
        n.value.to_string()
    } else {
        group_thousands(n.value)
    };
    Some(Rewrite {
        keep: 0,
        tokens: vec![Tok::exact(text)],
        len: n.len,
    })
}

/// Cheap check before the parsers run: can a rewrite start with `word`?
fn may_start(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_digit())
        || unit(word).is_some()
        || lookup(TEENS, word).is_some()
        || tens(word).is_some()
        || scale(word).is_some()
        || ordinal(word).is_some()
        || compound(word).is_some()
        || matches!(word, "klokken" | "klokka" | "kl" | "kl." | "uge")
        || PHONE_CUES.contains(&word)
        || CVR_CUES.contains(&word)
}

/// Format every number in `tokens`.
pub(super) fn format(tokens: Vec<Tok>) -> Vec<Tok> {
    let tokens = join_abbreviations(tokens);
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let starts = tokens[i].plain_word().is_some_and(|w| may_start(&w));
        let run = if starts {
            plain_run(&tokens, i)
        } else {
            Vec::new()
        };
        if run.is_empty() {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        }
        let prev = out.last().and_then(|t| match t {
            Tok::Word { text, .. } => Some(text.to_lowercase()),
            _ => None,
        });
        match rewrite(&run, 0, prev.as_deref()) {
            Some(r) if r.len > 0 => {
                out.extend(tokens[i..i + r.keep].iter().cloned());
                out.extend(r.tokens);
                i += r.keep + r.len;
            }
            _ => {
                out.push(tokens[i].clone());
                i += 1;
            }
        }
    }
    out
}

/// "kl" + "." and "kr" + "." from the speech model become one word, so the
/// passes can look past them.
fn join_abbreviations(tokens: Vec<Tok>) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    for tok in tokens {
        if tok == Tok::Punct('.') {
            if let Some(Tok::Word { text, .. }) = out.last_mut() {
                if matches!(text.to_lowercase().as_str(), "kl" | "kr" | "tlf" | "nr") {
                    text.push('.');
                    continue;
                }
            }
        }
        out.push(tok);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    fn value(s: &str) -> Option<u64> {
        let w = words(s);
        cardinal(&w, 0)
            .filter(|n| n.len == w.len())
            .map(|n| n.value)
    }

    #[test]
    fn parses_cardinals() {
        let cases: &[(&str, u64)] = &[
            ("tretten", 13),
            ("seksogtyve", 26),
            ("fem og fyrre", 45),
            ("enogtredive", 31),
            ("halvfems", 90),
            ("hundrede", 100),
            ("et hundrede", 100),
            ("to hundrede og halvtreds", 250),
            ("tretten hundrede og halvtreds", 1350),
            ("fem tusind", 5000),
            ("et tusind", 1000),
            ("to tusind og seksogtyve", 2026),
            ("tolv tusind fem hundrede", 12500),
            ("en million", 1_000_000),
            ("to millioner tre hundrede tusind", 2_300_000),
        ];
        for (text, expected) in cases {
            assert_eq!(value(text), Some(*expected), "{text}");
        }
        assert_eq!(value("en"), None);
        assert_eq!(value("et"), None);
    }

    #[test]
    fn groups_thousands() {
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1350), "1.350");
        assert_eq!(group_thousands(1_234_567), "1.234.567");
    }

    #[test]
    fn danish_digit_formats() {
        assert_eq!(danish_digits("3.5", false).as_deref(), Some("3,5"));
        assert_eq!(danish_digits("5.000", false).as_deref(), Some("5.000"));
        assert_eq!(danish_digits("5000", true).as_deref(), Some("5.000"));
        assert_eq!(danish_digits("5000", false).as_deref(), Some("5000"));
        assert_eq!(
            danish_digits("1,350,000", false).as_deref(),
            Some("1.350.000")
        );
        assert_eq!(danish_digits("1,350", true).as_deref(), Some("1.350"));
        assert_eq!(danish_digits("12,5", false).as_deref(), Some("12,5"));
        assert_eq!(danish_digits("1,350.50", true).as_deref(), Some("1.350,50"));
        assert_eq!(danish_digits("1.350,50", true).as_deref(), Some("1.350,50"));
        assert_eq!(danish_digits("v1.2", false), None);
    }
}
