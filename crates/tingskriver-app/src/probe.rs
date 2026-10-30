//! The test field on the dictation page shows what the keyboard actually
//! produced, so a manual test can tell a composed "é" (U+00E9) from a dead
//! key that was passed through as "´" + "e".

/// Accents that dead keys on a Danish keyboard produce on their own.
const SPACING_ACCENTS: &[char] = &['´', '`', '^', '¨', '~'];

/// One line per non-ASCII character (or ASCII accent) in `text`, as
/// "é U+00E9", in order of first appearance.
pub fn describe(text: &str) -> Vec<String> {
    let mut seen = Vec::new();
    for c in text.chars() {
        if (c.is_ascii() && !SPACING_ACCENTS.contains(&c)) || seen.contains(&c) {
            continue;
        }
        seen.push(c);
    }
    seen.iter()
        .map(|&c| {
            if is_combining(c) {
                format!("◌{c} U+{:04X} (kombinerende tegn)", c as u32)
            } else {
                format!("{c} U+{:04X}", c as u32)
            }
        })
        .collect()
}

/// A warning when a dead key was not composed with the following vowel:
/// a combining mark, or a spacing accent directly before a letter.
pub fn composition_warning(text: &str) -> Option<&'static str> {
    if text.chars().any(is_combining) {
        return Some("Teksten indeholder et kombinerende tegn: dødtasten blev ikke sat sammen med bogstavet.");
    }
    let chars: Vec<char> = text.chars().collect();
    let loose_accent = chars
        .windows(2)
        .any(|pair| SPACING_ACCENTS.contains(&pair[0]) && pair[1].is_alphabetic());
    loose_accent.then_some(
        "En accent står alene foran et bogstav. Tryk på dødtasten og derefter vokalen; ét tegn som é er forventet.",
    )
}

fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_danish_letters_once() {
        assert_eq!(describe("æøå æ"), vec!["æ U+00E6", "ø U+00F8", "å U+00E5"]);
        assert!(describe("plain ascii").is_empty());
    }

    #[test]
    fn composed_dead_keys_are_fine() {
        let text = "é ü ê è ñ";
        assert_eq!(describe(text).len(), 5);
        assert_eq!(composition_warning(text), None);
    }

    #[test]
    fn uncomposed_dead_keys_are_flagged() {
        assert!(composition_warning("e\u{301}").is_some());
        assert!(describe("e\u{301}")[0].contains("U+0301"));
        assert!(composition_warning("´e").is_some());
        assert!(composition_warning("~n").is_some());
        // An accent typed on its own (dead key then space) is intended.
        assert_eq!(composition_warning("´ og ~"), None);
    }

    #[test]
    fn altgr_symbols_are_listed() {
        assert_eq!(describe("@£$€{[]}|\\"), vec!["£ U+00A3", "€ U+20AC"]);
    }
}
