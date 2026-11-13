//! Texts for making, stopping and editing the summary of a meeting and for
//! the Opsummering settings tab, copied from the Tauri app
//! (`hooks/meeting-details/useSummaryGeneration.ts`, `SummaryPanel.tsx`,
//! `SummaryGeneratorButtonGroup.tsx`, `SummaryUpdaterButtonGroup.tsx`,
//! `ModelSettingsModal.tsx`). Texts marked "GPUI" are new: the editor and
//! the fields the settings tab needs without a modal.

catalogue! {
    // useSummaryGeneration.ts: checks before a run.
    SUMMARY_NO_TRANSCRIPT = "Der er intet transskript at opsummere";
    SUMMARY_OLLAMA_NONE = "Der er ingen Ollama-modeller. Hent gemma3:1b under modelindstillingerne.";
    SUMMARY_BUILTIN_NO_MODEL = "Der er ikke valgt en indbygget AI-model. Vælg en under indstillingerne.";
    SUMMARY_BUILTIN_NOT_READY = "Den indbyggede AI-model er ikke klar. Tjek modelindstillingerne.";
    SUMMARY_MODEL_CHECK_FAILED = "Opsummeringsmodellen kunne ikke kontrolleres. Tjek modelindstillingerne.";
    SUMMARY_NO_MODEL = "Der er ikke valgt en model til opsummering. Vælg en under Indstillinger > Opsummering.";
    SUMMARY_SETTINGS_UNREADABLE = "Indstillingerne for opsummering kunne ikke læses. Prøv igen.";
    SUMMARY_LANGUAGE_UNREADABLE = "Det gemte sprog til opsummering kunne ikke indlæses";
    SUMMARY_LANGUAGE_UNREADABLE_HELP = "Bruger automatisk sprogvalg denne gang.";
    SUMMARY_BILINGUAL = "Transskriptet er tosproget";
    SUMMARY_BILINGUAL_HELP = "Vælg sprog til opsummeringen selv, hvis automatisk sprogvalg vælger forkert.";
    // useSummaryGeneration.ts: toasts while it runs.
    SUMMARY_MAKING = "Laver opsummering…";
    SUMMARY_REMAKING = "Genskaber opsummering…";
    SUMMARY_USING = "Bruger {value}";
    SUMMARY_READY = "Opsummeringen er klar";
    SUMMARY_READY_FILTERED = "Mødets opsummering er klar. Modellens tankegang er filtreret fra noterne.";
    SUMMARY_FALLBACK = "Opsummeringen er lavet med reservevalg";
    SUMMARY_FALLBACK_HELP = "Normalisering til engelsk mislykkedes, så den oprindelige, rensede opsummering blev beholdt.";
    SUMMARY_FAILED_NEW = "Opsummeringen kunne ikke laves";
    SUMMARY_FAILED_AGAIN = "Opsummeringen kunne ikke genskabes";
    SUMMARY_PREVIOUS_RESTORED = "{value}. Din tidligere opsummering er gendannet.";
    SUMMARY_STOPPED = "Opsummeringen er stoppet";
    SUMMARY_STOPPED_HELP = "Du kan lave en ny opsummering, når du vil";
    SUMMARY_ALMOST_DONE = "Opsummeringen er næsten færdig";
    SUMMARY_ALMOST_DONE_HELP = "Venter på det seneste resultat.";
    SUMMARY_STOP_FAILED = "Opsummeringen kunne ikke stoppes";
    SUMMARY_STOP_FAILED_HELP = "Opsummeringen kører stadig. Venter på den seneste status.";
    SUMMARY_NO_RESULT = "Opsummeringen blev færdig uden synligt indhold. Prøv igen.";
    SUMMARY_START_FAILED = "Opsummeringen kunne ikke startes.";
    // SummaryGeneratorButtonGroup.tsx.
    SUMMARY_STOP = "Stop";
    SUMMARY_STOP_TITLE = "Stop opsummeringen";
    SUMMARY_REGENERATE_TITLE = "Lav opsummeringen om";
    SUMMARY_GENERATE_TITLE = "Lav opsummering";
    SUMMARY_TEMPLATE_TITLE = "Vælg skabelon til opsummeringen";
    SUMMARY_MODEL_TITLE = "Vælg AI-model til opsummeringer";
    SUMMARY_TEMPLATE_CHOSEN = "Skabelon valgt";
    SUMMARY_TEMPLATE_CHOSEN_HELP = "Bruger skabelonen \"{value}\" til opsummeringen";
    SUMMARY_TEMPLATES_FAILED = "Skabelonerne kunne ikke hentes";
    // SummaryPanel.tsx: the language of one meeting.
    SUMMARY_LANGUAGE_AUTO_SUBTITLE = "Bruger det sprog, der oftest tales i transskriptet";
    SUMMARY_LANGUAGE_LOCAL_SUBTITLE = "Gemmes på denne computer for møder uden optagemappe";
    SUMMARY_LANGUAGE_SAVED_LOCALLY = "Opsummeringssproget er gemt på denne computer";
    SUMMARY_LANGUAGE_SAVED_LOCALLY_HELP = "Mødet har ingen optagemappe, så valget kan ikke gemmes sammen med mødet.";
    SUMMARY_LANGUAGE_SAVE_FAILED = "Opsummeringssproget kunne ikke gemmes";
    SUMMARY_LANGUAGE_SAVE_FAILED_HELP = "Prøv igen.";
    SUMMARY_LANGUAGE_READ_FAILED = "Det gemte opsummeringssprog kunne ikke hentes";
    SUMMARY_LANGUAGE_READ_FAILED_HELP = "Bruger automatisk sprogvalg, indtil mødets oplysninger kan læses.";
    // SummaryUpdaterButtonGroup.tsx and useMeetingData.ts.
    SUMMARY_SAVING = "Gemmer…";
    SUMMARY_SAVE_DIRTY = "Gem ændringer";
    SUMMARY_SAVE_TITLE = "Gem ændringerne";
    SUMMARY_SAVED = "Ændringerne er gemt";
    SUMMARY_SAVE_FAILED = "Ændringerne kunne ikke gemmes";
    // GPUI: the plain text editor that replaces BlockNote.
    SUMMARY_EDIT = "Rediger";
    SUMMARY_EDIT_TITLE = "Rediger opsummeringen";
    SUMMARY_EDIT_CANCEL = "Fortryd";
    SUMMARY_EDIT_CANCEL_TITLE = "Forkast ændringerne";
    SUMMARY_EDITOR_LABEL = "Opsummeringens tekst";
    SUMMARY_EDITOR_HELP = "Skriv i Markdown: # til overskrifter, - til punkter og ** til fed skrift.";
    SUMMARY_EDIT_EMPTY = "Opsummeringen kan ikke være tom. Skriv noget, eller fortryd ændringerne.";
    SUMMARY_EDIT_RUNNING = "Opsummeringen skrives lige nu, så den kan ikke redigeres.";
    SUMMARY_EDIT_CHANGED = "Opsummeringen er ændret, siden du begyndte at redigere. Dine ændringer er ikke gemt.";

    // Opsummering settings tab: ModelSettingsModal.tsx.
    SUMMARY_SETTINGS_SAVED = "Indstillingerne for opsummering er gemt";
    SUMMARY_SETTINGS_SAVE_FAILED = "Indstillingerne for opsummering kunne ikke gemmes";
    SUMMARY_MODELS_LOADING = "Henter modeller …";
    SUMMARY_MODELS_NONE = "Ingen modeller fundet";
    SUMMARY_MODELS_FAILED = "Modellerne kunne ikke hentes";
    SUMMARY_MODELS_KEY_NEEDED = "Gem en API-nøgle for at hente modellerne.";
    SUMMARY_OLLAMA_ENDPOINT = "Ollama-adresse";
    SUMMARY_OLLAMA_ENDPOINT_HELP = "Lad feltet stå tomt for at bruge den lokale Ollama.";
    SUMMARY_KEY = "API-nøgle";
    SUMMARY_KEY_SAVED = "Der er gemt en nøgle. Skriv en ny for at erstatte den.";
    SUMMARY_KEY_MISSING = "Der er ikke gemt en nøgle endnu.";
    SUMMARY_KEY_PLACEHOLDER = "Indsæt nøglen her";
    SUMMARY_KEY_SAVE = "Gem nøgle";
    SUMMARY_KEY_SAVED_TOAST = "Nøglen er gemt";
    SUMMARY_KEY_SAVE_FAILED = "Nøglen kunne ikke gemmes";
    SUMMARY_CUSTOM_ENDPOINT = "Adresse til serveren";
    SUMMARY_CUSTOM_ENDPOINT_PLACEHOLDER = "http://localhost:8000/v1";
    SUMMARY_CUSTOM_MODEL = "Modelnavn";
    SUMMARY_CUSTOM_SAVE = "Gem serveren";
    SUMMARY_CUSTOM_REQUIRED = "Adressen og modelnavnet skal udfyldes.";
    SUMMARY_BUILTIN_NOT_DOWNLOADED = "Ikke hentet";
    SUMMARY_BUILTIN_DOWNLOADING = "Henter";
    SUMMARY_BUILTIN_HELP = "Vælg en model, der er hentet. Nye modeller hentes i den nuværende app.";
    SUMMARY_AUTO_SAVE_FAILED = "Indstillingen kunne ikke gemmes";
    SUMMARY_AUTO_UNREADABLE = "Indstillingen kunne ikke læses. Kontakten viser standardværdien.";
    SUMMARY_PINNED_STANDARD = "Standard: {value}. Klik på det igen for at fjerne standarden. Højst 5 hurtigvalg.";
    SUMMARY_PINNED_NONE = "Klik på et sprog for at bruge det som standard. Højst 5 hurtigvalg.";
    SUMMARY_ADD_LANGUAGE = "Tilføj sprog";
    SUMMARY_LANGUAGE_USE = "Brug {value} som standard";
    SUMMARY_LANGUAGE_UNUSE = "Fjern {value} som standard";
    SUMMARY_LANGUAGE_REMOVE = "Fjern {value} fra listen";
    SUMMARY_LANGUAGES_UNREADABLE = "Hurtigvalgene kunne ikke læses. Listen er tom, til du gemmer en ændring.";

    // Forbedr: RetranscribeDialog.tsx.
    IMPROVE_TITLE = "Transskribér mødet igen";
    IMPROVE_LANGUAGE = "Sprog";
    IMPROVE_LANGUAGE_AUTO = "Find sproget automatisk";
    IMPROVE_LANGUAGE_HELP = "Vælg et bestemt sprog for bedre træfsikkerhed, eller lad appen finde sproget selv";
    IMPROVE_PARAKEET_NOTE = "Parakeet understøtter ikke valg af sprog. Sproget findes altid automatisk.";
    IMPROVE_MODEL = "Model";
    IMPROVE_MODEL_HELP = "Den model, der er valgt under Indstillinger > Transskription";
    IMPROVE_START = "Start";
    IMPROVE_CANCEL = "Annullér";
    IMPROVE_STOP = "Stop";
    IMPROVE_CLOSE = "Luk";
    IMPROVE_RETRY = "Prøv igen";
    IMPROVE_FOLDER_MISSING = "Mødets mappe blev ikke fundet. Luk dialogen, og åbn mødet igen.";
    IMPROVE_BUSY = "Der kører allerede en gentranskribering. Vent til den er færdig.";
    IMPROVE_FAILED_HELP = "Prøv igen, eller vælg en anden model.";
    IMPROVE_DONE = "Gentranskriberingen er færdig. {value} afsnit er oprettet.";
    IMPROVE_WARNING = "Det nye transskript erstatter det nuværende, når det er færdigt.";
}

/// "Bruger udbyder/model" under the toast that a run started.
pub fn summary_using(provider: &str, model: &str) -> String {
    SUMMARY_USING.replace("{value}", &format!("{provider}/{model}"))
}

/// Tauri's description of a restored summary after a failed run.
pub fn summary_previous_restored(message: &str) -> String {
    SUMMARY_PREVIOUS_RESTORED.replace("{value}", message)
}

/// "Bruger skabelonen "X" til opsummeringen".
pub fn template_chosen(name: &str) -> String {
    SUMMARY_TEMPLATE_CHOSEN_HELP.replace("{value}", name)
}

/// The pinned default language line of the Opsummering tab.
pub fn pinned_standard(label: &str) -> String {
    SUMMARY_PINNED_STANDARD.replace("{value}", label)
}

pub fn language_use(label: &str) -> String {
    SUMMARY_LANGUAGE_USE.replace("{value}", label)
}

pub fn language_unuse(label: &str) -> String {
    SUMMARY_LANGUAGE_UNUSE.replace("{value}", label)
}

pub fn language_remove(label: &str) -> String {
    SUMMARY_LANGUAGE_REMOVE.replace("{value}", label)
}

/// "Gentranskriberingen er færdig. N afsnit er oprettet."
pub fn improve_done(segments: usize) -> String {
    IMPROVE_DONE.replace("{value}", &segments.to_string())
}
