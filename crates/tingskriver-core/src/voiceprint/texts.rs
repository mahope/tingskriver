//! The explanation of voiceprints, in Danish and English, shown together on
//! the settings page (and the privacy page of #84 when it exists). The
//! Danish text is in the UI catalogue ([`crate::strings`]); the English text
//! is here. Both say the same: what a voiceprint is, that it is biometric
//! data under GDPR article 9, that it stays on this PC, what consent means
//! and how to delete.

use crate::strings;

/// Bump when the consent text changes. Stored with every consent, so it is
/// known which wording a person agreed to.
pub const CONSENT_TEXT_VERSION: u32 = 1;

pub const EN_TITLE: &str = "Voiceprints";
pub const EN_STATUS_OFF: &str = "Off. Tingskriver remembers no voices.";
pub const EN_STATUS_ON: &str =
    "On. Tingskriver only remembers voices of people who have given consent.";
pub const EN_WHAT: &str = "A voiceprint is a series of numbers describing how a particular voice sounds. With it, Tingskriver can suggest the name of a participant who comes back, so you do not have to type it every time.";
pub const EN_LAW: &str = "A voiceprint can recognise a person, so it is biometric data. GDPR (article 9) gives it especially strong protection, so you may only keep one with the person's consent.";
pub const EN_LOCAL: &str = "Voiceprints are kept only on this PC, in Windows Credential Manager. They are never sent anywhere, never synchronised, and never part of exports, deliveries, log files, diagnostic reports or backups.";
pub const EN_CONSENT: &str = "Consent: you may only keep a person's voiceprint when the person has been told and has said yes. You confirm that in the app for each person. Consent can be withdrawn at any time, and then the person's voiceprints are deleted immediately.";
pub const EN_DELETE: &str = "Deleting: you can forget one person or everyone at once. Voiceprints not recognised for 30, 90 or 180 days (your choice, default 90) are deleted automatically. Uninstalling with \"delete data\" removes them too.";
pub const EN_SUGGEST: &str = "Tingskriver never names anyone automatically. You get a suggestion that you confirm with one click. The limits for when two voices are alike enough were measured on English and are not calibrated for Danish. Suggestions can therefore be wrong.";

/// One paragraph in both languages.
pub struct Paragraph {
    pub da: &'static str,
    pub en: &'static str,
}

/// The explanation, in reading order, Danish and English side by side.
pub fn explanation() -> [Paragraph; 6] {
    [
        Paragraph {
            da: strings::VOICEPRINT_WHAT,
            en: EN_WHAT,
        },
        Paragraph {
            da: strings::VOICEPRINT_LAW,
            en: EN_LAW,
        },
        Paragraph {
            da: strings::VOICEPRINT_LOCAL,
            en: EN_LOCAL,
        },
        Paragraph {
            da: strings::VOICEPRINT_CONSENT,
            en: EN_CONSENT,
        },
        Paragraph {
            da: strings::VOICEPRINT_DELETE,
            en: EN_DELETE,
        },
        Paragraph {
            da: strings::VOICEPRINT_SUGGEST,
            en: EN_SUGGEST,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explanation_covers_the_required_points_in_both_languages() {
        let (da, en): (Vec<_>, Vec<_>) = explanation().iter().map(|p| (p.da, p.en)).unzip();
        let (da, en) = (da.join(" ").to_lowercase(), en.join(" ").to_lowercase());
        for needle in [
            "stemmeaftryk er",
            "biometri",
            "gdpr",
            "artikel 9",
            "denne pc",
            "samtykke",
            "slettes",
            "glemme",
            "ikke kalibreret til dansk",
            "ét klik",
        ] {
            assert!(da.contains(needle), "Danish text lacks: {needle}");
        }
        for needle in [
            "voiceprint is",
            "biometric",
            "gdpr",
            "article 9",
            "this pc",
            "consent",
            "deleted",
            "forget",
            "not calibrated for danish",
            "one click",
        ] {
            assert!(en.contains(needle), "English text lacks: {needle}");
        }
    }

    #[test]
    fn texts_have_no_dashes_or_legacy_brand() {
        for p in explanation() {
            for t in [p.da, p.en] {
                assert!(!t.contains('\u{2014}'));
                assert!(!t.to_lowercase().contains("meetily"));
            }
        }
    }
}
