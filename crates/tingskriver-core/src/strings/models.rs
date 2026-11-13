//! Modelhåndtering under Indstillinger > Transskription (#174), copied from
//! the Tauri model managers (`ParakeetModelManager.tsx`,
//! `WhisperModelManager.tsx`, `lib/parakeet.ts`, `lib/whisper.ts`). Texts
//! marked "GPUI" are new: sizes, source and licence, checksum status, the
//! reasons a model cannot be deleted and the readable download errors.

catalogue! {
    // TranscriptSettings.tsx:117.
    MODELS_ENGINE_PLACEHOLDER = "Vælg motor";
    // ParakeetModelManager.tsx: card, buttons and progress.
    MODEL_RECOMMENDED = "Anbefalet";
    MODEL_SELECTED = "Valgt";
    MODEL_READY = "Klar";
    MODEL_DOWNLOAD = "Hent";
    MODEL_RETRY = "Prøv igen";
    MODEL_DOWNLOAD_AGAIN = "Hent igen";
    MODEL_DELETE = "Slet";
    MODEL_DELETE_LABEL = "Slet model for at frigøre plads";
    MODEL_DOWNLOADING = "Henter…";
    MODEL_CANCELLING = "Annullerer…";
    MODEL_CANCEL = "Annullér";
    MODEL_CANCEL_LABEL = "Annullér hentning";
    MODEL_CANCEL_STARTED = "Annullering igangsat";
    MODEL_USING = "Bruger {value} til transskription";
    MODELS_LOAD_FAILED = "Kunne ikke hente modellerne. Genstart appen, og prøv igen.";
    MODEL_CANCEL_PENDING = "Hentningen er ved at stoppe. Du kan prøve igen, når oprydningen er færdig.";
    MODEL_CANCEL_FAILED = "Kunne ikke annullere hentningen";
    MODEL_DOWNLOAD_FAILED_GENERIC = "Hentningen mislykkedes. Tjek internetforbindelsen, og prøv igen.";
    MODEL_DELETE_FAILED = "Sletningen mislykkedes. Prøv igen.";
    MODEL_PARAKEET_FAST = "Lynhurtig";
    MODEL_PARAKEET_FAST_TAGLINE = "Realtid • Hurtigst, stor nøjagtighed";
    MODEL_PARAKEET_COMPACT = "Kompakt";
    MODEL_PARAKEET_COMPACT_TAGLINE = "Realtid • Mindre størrelse";
    MODEL_PARAKEET_PRECISE = "Præcis";
    MODEL_PARAKEET_PRECISE_TAGLINE = "20 gange realtid • Højere nøjagtighed";
    // lib/whisper.ts:getModelPerformanceBadge.
    MODEL_WHISPER_F16 = "Fuld præcision";
    MODEL_WHISPER_Q5_1 = "Balanceret+";
    MODEL_WHISPER_Q5_0 = "Balanceret";
    MODEL_WHISPER_Q4_0 = "Hurtig";
    // GPUI: nothing is fetched by opening the tab.
    MODELS_LOADING = "Læser de modeller, der ligger på computeren…";
    MODELS_NO_HOST = "Modelhåndtering er ikke tilgængelig, fordi appens datamappe ikke kunne åbnes.";
    MODELS_NONE_INSTALLED = "Der er ikke hentet nogen model endnu. Vælg Hent ved en model for at komme i gang. Der hentes ikke noget, før du selv vælger det.";
    MODEL_WILL_DOWNLOAD = "Hentes fra {source} ({size}) først, når du vælger Hent.";
    MODEL_DETAILS = "Størrelse {size} · Kilde {source} · Licens {license} · Revision {revision}";
    MODEL_UNKNOWN = "ukendt";
    MODEL_CHECK_UNKNOWN = "Kontrolsum: ikke kontrolleret i denne session";
    MODEL_CHECK_CHECKING = "Kontrollerer kontrolsum…";
    MODEL_CHECK_PASSED = "Kontrolsum (SHA-256) bekræftet";
    MODEL_CHECK_FAILED = "Kontrolsummen passer ikke. Slet modellen, og hent den igen.";
    MODEL_CHECK_UNSUPPORTED = "Kontrolsum: ikke tilgængelig for denne model";
    MODEL_CHECK = "Kontrollér";
    MODEL_CHECK_LABEL = "Kontrollér modellens kontrolsum";
    MODEL_CORRUPTED = "Modelfilen er beskadiget eller ufuldstændig. Slet den, eller hent den igen.";
    MODEL_SELECT_SAVE_FAILED = "Valget kunne ikke gemmes. Din tidligere model er bevaret.";
    MODEL_SELECT_NO_DATABASE = "Valget kan først gemmes, når databasen er åben. Din tidligere model er bevaret.";
    MODEL_DELETED = "{value} er slettet";
    MODEL_ERR_DISK = "Der er ikke plads nok på disken. Modellen fylder {size}. Frigør plads, og prøv igen.";
    MODEL_ERR_DISK_UNKNOWN = "Disken er fuld, eller modellen kunne ikke skrives. Frigør plads, og prøv igen.";
    MODEL_ERR_NETWORK = "Hentningen blev afbrudt, eller kilden svarede ikke. Tjek internetforbindelsen, og prøv igen. Det, der allerede er hentet, gemmes, så hentningen fortsætter, hvor den slap.";
    MODEL_ERR_NOT_READY = "Modelhåndteringen er ikke klar. Genstart appen, og prøv igen.";
    MODEL_KEPT_EXISTING = "Den eksisterende modelfil er ikke ændret.";
    MODEL_BUSY_RECORDING = "Kan ikke slettes, mens en optagelse kører. Stop optagelsen først.";
    MODEL_BUSY_DICTATION = "Kan ikke slettes, mens du dikterer. Afslut dikteringen først.";
    MODEL_BUSY_TRANSCRIBING = "Kan ikke slettes, mens et møde bliver transskriberet. Vent, til det er færdigt.";
    MODEL_DELETE_TITLE = "Slet model";
    MODEL_DELETE_CONFIRM = "Slet {value}? Modellen fjernes fra computeren, og du skal hente den igen for at bruge den.";
    MODEL_DELETE_CONFIRM_SELECTED = " Det er din valgte model, så du skal vælge eller hente en anden, før du kan transskribere.";
    MODEL_PROGRESS_LABEL = "Henter {name}: {percent} procent";
    MODEL_ENGINE_LABEL = "Transskriptionsmotor";
    MODEL_CHOOSE = "Vælg";
    MODEL_SELECTED_MISSING = "Din valgte model ligger ikke på computeren. Hent den igen, eller vælg en anden model.";
    // ParakeetModelManager.tsx:116: the toast after "Annullér".
    MODEL_CANCELLED = "Hentningen af {value} er annulleret";
}

/// "Bruger Lynhurtig til transskription".
pub fn model_using(name: &str) -> String {
    MODEL_USING.replace("{value}", name)
}

/// "Hentes fra huggingface (670 MB) først, når du vælger Hent."
pub fn model_will_download(source: &str, size: &str) -> String {
    MODEL_WILL_DOWNLOAD
        .replace("{source}", source)
        .replace("{size}", size)
}

/// "Størrelse 670 MB · Kilde … · Licens … · Revision …". Unsupported
/// metadata is passed as `None` and shown as unknown.
pub fn model_details(
    size: &str,
    source: Option<&str>,
    license: Option<&str>,
    revision: Option<&str>,
) -> String {
    MODEL_DETAILS
        .replace("{size}", size)
        .replace("{source}", source.unwrap_or(MODEL_UNKNOWN))
        .replace("{license}", license.unwrap_or(MODEL_UNKNOWN))
        .replace("{revision}", revision.unwrap_or(MODEL_UNKNOWN))
}

pub fn model_err_disk(size: &str) -> String {
    MODEL_ERR_DISK.replace("{size}", size)
}

pub fn model_cancelled(name: &str) -> String {
    MODEL_CANCELLED.replace("{value}", name)
}

pub fn model_deleted(name: &str) -> String {
    MODEL_DELETED.replace("{value}", name)
}

pub fn model_delete_confirm(name: &str, selected: bool) -> String {
    let mut text = MODEL_DELETE_CONFIRM.replace("{value}", name);
    if selected {
        text.push_str(MODEL_DELETE_CONFIRM_SELECTED);
    }
    text
}

pub fn model_progress_label(name: &str, percent: u8) -> String {
    MODEL_PROGRESS_LABEL
        .replace("{name}", name)
        .replace("{percent}", &percent.to_string())
}

/// `formatFileSize` of `lib/parakeet.ts` with a Danish decimal comma:
/// "670 MB", "2,6 GB".
pub fn model_size(size_mb: f64) -> String {
    if size_mb >= 1000.0 {
        format!("{:.1} GB", size_mb / 1000.0).replace('.', ",")
    } else {
        format!("{} MB", size_mb.round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_danish_decimals_and_units() {
        assert_eq!(model_size(670.0), "670 MB");
        assert_eq!(model_size(2554.0), "2,6 GB");
        assert_eq!(model_size(999.6), "1000 MB");
    }

    #[test]
    fn unknown_metadata_is_shown_as_unknown() {
        let line = model_details("74 MB", Some("huggingface"), None, None);
        assert!(line.contains("Licens ukendt"));
        assert!(line.contains("Revision ukendt"));
        assert!(line.contains("Kilde huggingface"));
    }

    #[test]
    fn delete_confirmation_names_the_selected_model() {
        assert!(!model_delete_confirm("base", false).contains("valgte"));
        assert!(model_delete_confirm("base", true).contains("valgte model"));
    }
}
