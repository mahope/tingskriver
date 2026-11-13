//! Import texts, copied from the Tauri app
//! (`frontend/src/components/ImportAudio/ImportAudioDialog.tsx`,
//! `ImportDropOverlay.tsx`, `contexts/ImportDialogContext.tsx`,
//! `hooks/useImportAudio.ts`).

catalogue! {
    IMPORT_TRANSCRIPT_HINT = "Vælg en .vtt- eller .srt-fil fra Teams, Zoom eller Meet, eller træk den hertil";
    IMPORT_DIALOG_TITLE = "Importér lydfil";
    IMPORT_DIALOG_BUSY_TITLE = "Importerer lyd…";
    IMPORT_DIALOG_FAILED_TITLE = "Importen mislykkedes";
    IMPORT_DIALOG_LEAD = "Importér en lydfil for at oprette et nyt møde med transskript";
    IMPORT_DIALOG_BUSY_LEAD = "Behandler lyd…";
    IMPORT_DIALOG_FAILED_LEAD = "Noget gik galt under importen";
    IMPORT_PICK_FILE = "Vælg lydfil";
    IMPORT_PICK_OTHER = "Vælg en anden fil";
    IMPORT_VALIDATING = "Tjekker filen…";
    IMPORT_FORMATS = "MP4, M4A, WAV, MP3, FLAC, OGG, AAC, MKV, WebM, WMA";
    IMPORT_MEETING_TITLE = "Mødetitel";
    IMPORT_TITLE_PLACEHOLDER = "Skriv en mødetitel";
    IMPORT_MORE_OPTIONS = "Flere valg";
    IMPORT_LANGUAGE = "Sprog";
    IMPORT_MODEL = "Model";
    IMPORT_LOADING_MODELS = "Henter modeller…";
    IMPORT_PARAKEET_LANGUAGE = "Parakeet understøtter ikke valg af sprog. Sproget registreres automatisk.";
    IMPORT_START = "Importér";
    IMPORT_CANCEL = "Annullér";
    IMPORT_CLOSE = "Luk";
    IMPORT_RETRY = "Prøv igen";
    IMPORT_CANCELLED = "Importen er annulleret";
    IMPORT_FAILED_TOAST = "Importen mislykkedes";
    IMPORT_BETA_OFF_TITLE = "Beta-funktionen er slået fra";
    IMPORT_BETA_OFF_TEXT = "Slå \"Importér lyd og transskribér igen\" til under Indstillinger > Beta for at bruge den.";
    IMPORT_BUSY = "Der er allerede en import i gang. Vent, til den er færdig.";
    IMPORT_SUBTITLES_ONLY = "Kun .vtt- og .srt-filer kan importeres";
    IMPORT_TRANSCRIPT_DONE = "Transskriptet er importeret";
    IMPORT_TRANSCRIPT_EXISTS = "Transskriptet er allerede importeret";
    IMPORT_TRANSCRIPT_EXISTS_TEXT = "Det eksisterende møde er åbnet.";
    IMPORT_TRANSCRIPT_FAILED = "Transskriptet kunne ikke importeres";
    IMPORT_TRANSCRIPT_FILE_FAILED = "Filen kunne ikke importeres";
    IMPORT_SUMMARY_FAILED = "Referatet kunne ikke laves";
    IMPORT_SUMMARY_FAILED_TEXT = "Mødet og transskriptet er gemt. Brug \"Lav referat igen\" på mødet.";
    IMPORT_NO_DATABASE = "Databasen er ikke åben endnu. Prøv igen om et øjeblik.";
    IMPORT_NO_HOST = "Import er ikke tilgængelig, fordi appens datamappe mangler.";
    IMPORT_FILE_CHECK_FAILED = "Filen kunne ikke kontrolleres";
    IMPORT_DROP_TITLE = "Slip filen for at importere den";
    IMPORT_DROP_FORMATS = "MP4, M4A, WAV, MP3, FLAC, OGG, AAC, MKV, WebM, WMA eller undertekster (.vtt, .srt)";
    IMPORT_DROP_UNSUPPORTED = "Filtypen understøttes ikke. Slip en lydfil eller en undertekstfil (.vtt, .srt).";
    IMPORT_BETA_SAVE_FAILED = "Indstillingen kunne ikke gemmes";
}

/// "Importen er færdig. 12 afsnit er oprettet." (`handleImportComplete`).
pub fn import_done(segments: usize) -> String {
    format!("Importen er færdig. {segments} afsnit er oprettet.")
}

/// "⚡ Parakeet: parakeet-tdt (640 MB)" (the model list, `ImportAudioDialog.tsx`).
pub fn model_label(icon_name: &str, name: &str, size_mb: impl std::fmt::Display) -> String {
    format!("{icon_name}: {name} ({size_mb} MB)")
}

/// "42 %" (the progress bar).
pub fn percent(value: impl std::fmt::Display) -> String {
    format!("{value} %")
}

/// "12 afsnit. Referatet bliver lavet med den valgte udbyder."
pub fn transcript_done_text(segments: usize) -> String {
    format!("{segments} afsnit. Referatet bliver lavet med den valgte udbyder.")
}

/// Tauri's `formatFileSize`: one decimal with a comma.
pub fn file_size(bytes: u64) -> String {
    let one = |value: f64, unit: &str| format!("{value:.1}").replace('.', ",") + " " + unit;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        one(bytes as f64 / 1024.0, "KB")
    } else if bytes < 1024 * 1024 * 1024 {
        one(bytes as f64 / (1024.0 * 1024.0), "MB")
    } else {
        one(bytes as f64 / (1024.0 * 1024.0 * 1024.0), "GB")
    }
}

/// Tauri's `formatDuration`: `m:ss`, or `h:mm:ss` from one hour.
pub fn file_duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (hours, minutes, secs) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_and_durations_match_tauri() {
        assert_eq!(super::file_size(512), "512 B");
        assert_eq!(super::file_size(1536), "1,5 KB");
        assert_eq!(super::file_size(5 * 1024 * 1024), "5,0 MB");
        assert_eq!(super::file_size(3 * 1024 * 1024 * 1024), "3,0 GB");
        assert_eq!(super::file_duration(65.9), "1:05");
        assert_eq!(super::file_duration(3725.0), "1:02:05");
    }
}
