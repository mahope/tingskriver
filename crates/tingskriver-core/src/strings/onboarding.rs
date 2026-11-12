//! Onboarding texts, copied verbatim from the Tauri app
//! (`frontend/src/components/onboarding/`):
//!
//! - `shared/ProgressIndicator.tsx:14`: STEP_OF
//! - `steps/WelcomeStep.tsx:11-24`: WELCOME_*
//! - `steps/DownloadProgressStep.tsx`: 26-27 and 503-515 (title, lead, cards,
//!   size), 425-439 (card status; "Henter" is the spinner's aria-label,
//!   "Hentet" names the check mark, which has no label in Tauri), 450
//!   (progress bar name), 467-474 (amounts), 481-493 (error box), 540-542
//!   (summary note), 561-566 (button), 376-377 (continue toast), 397-398 (completion).

catalogue! {
    STEP_OF = "Trin {current} af {total}";
    WELCOME_TITLE = "Velkommen til Tingskriver";
    WELCOME_LEAD = "Optag, transskribér og opsummér dine møder. Alt kører lokalt på din computer, og dine optagelser forlader aldrig den.";
    WELCOME_NOTE = "Først skal du hente en transskriptionsmodel. Det tager få minutter.";
    WELCOME_START = "Kom i gang";
    DOWNLOAD_TITLE = "Hent transskriptionsmodel";
    DOWNLOAD_LEAD = "Modellen kører på din computer og skal hentes, før du kan optage. Du kan fortsætte, så snart den er klar.";
    TRANSCRIPTION_CARD = "Transskriptionsmodel";
    SUMMARY_CARD = "Opsummeringsmodel";
    TRANSCRIPTION_SIZE = "~670 MB";
    WAITING = "Venter…";
    DOWNLOADING = "Henter";
    DONE = "Hentet";
    FAILED = "Mislykkedes";
    CANCELLED = "Annulleret";
    PROGRESS_LABEL = "Hentning af {model}";
    AMOUNT = "{done} {unit} / {total} {unit}";
    SPEED = "{speed} {unit}/s";
    PERCENT = "{value}%";
    UNIT_MB = "MB";
    UNIT_MIB = "MiB";
    DOWNLOAD_CANCELLED = "Hentningen blev annulleret";
    DOWNLOAD_FAILED = "Hentningen mislykkedes";
    RETRY = "Prøv igen";
    SUMMARY_CONTINUE_TITLE = "Du kan fortsætte, mens opsummeringsmodellen hentes";
    SUMMARY_CONTINUE_TEXT = "Hentningen fortsætter i baggrunden.";
    PREPARING = "Gør klar…";
    FETCHING_MODEL = "Henter model…";
    FINISH = "Færdig";
    BACKGROUND_TITLE = "Hentningen fortsætter i baggrunden";
    BACKGROUND_TEXT = "Du kan begynde at bruge appen. Optagelse er klar, når talegenkendelsen er hentet.";
    COMPLETE_FAILED_TITLE = "Opsætningen kunne ikke gennemføres";
    COMPLETE_FAILED_TEXT = "Prøv igen.";
}

/// "Trin 1 af 2" above the progress segments.
pub fn step_of(current: usize, total: usize) -> String {
    STEP_OF
        .replace("{current}", &current.to_string())
        .replace("{total}", &total.to_string())
}

/// "Hentning af transskriptionsmodel": the progress bar's accessible name.
pub fn progress_label(card: &str) -> String {
    PROGRESS_LABEL.replace("{model}", &card.to_lowercase())
}

/// "0.0 MB / 670.0 MB", one decimal like Tauri's `toFixed(1)`.
pub fn amount(done_mb: f64, total_mb: f64, unit: &str) -> String {
    AMOUNT
        .replace("{done}", &format!("{done_mb:.1}"))
        .replace("{total}", &format!("{total_mb:.1}"))
        .replace("{unit}", unit)
}

/// "4.2 MB/s".
pub fn speed(mb_per_second: f64, unit: &str) -> String {
    SPEED
        .replace("{speed}", &format!("{mb_per_second:.1}"))
        .replace("{unit}", unit)
}

/// "42%", rounded like Tauri's `Math.round`.
pub fn percent(value: f64) -> String {
    PERCENT.replace("{value}", &format!("{}", value.round() as i64))
}

#[cfg(test)]
mod tests {
    #[test]
    fn templates_fill_in_their_values() {
        assert_eq!(super::step_of(1, 2), "Trin 1 af 2");
        assert_eq!(
            super::progress_label(super::TRANSCRIPTION_CARD),
            "Hentning af transskriptionsmodel"
        );
        assert_eq!(super::amount(0.0, 670.0, "MB"), "0.0 MB / 670.0 MB");
        assert_eq!(super::speed(4.25, "MiB"), "4.2 MiB/s");
        assert_eq!(super::percent(41.6), "42%");
    }
}
