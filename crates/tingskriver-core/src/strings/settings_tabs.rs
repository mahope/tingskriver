//! The settings tabs Optagelser, Transskription, Opsummering and Beta
//! (docs/design-paritet.md, Job 6), copied verbatim from the Tauri app
//! (`frontend/src/components/`, file:line per block). Texts marked "GPUI"
//! are new: the explanations for controls this app cannot use yet, the
//! Måling entry and the sub-page header.

catalogue! {
    // Optagelser: RecordingSettings.tsx:145-218, DeviceSelection.tsx:243-365.
    RECORDING_TITLE = "Optagelser";
    RECORDING_HELP = "Vælg, hvordan lyden fra dine møder gemmes.";
    RECORDING_SAVED_IN = "Gemmes i";
    RECORDING_DEFAULT_FOLDER = "Standardmappen";
    RECORDING_ALWAYS_SAVED = "Lyden gemmes altid, fordi transskriptionen laves ud fra lydfilen efter mødet. Hvert møde får sin egen mappe med audio.mp4. Vil du ikke beholde lyden, kan du slette mødets mappe.";
    RECORDING_START_NOTICE = "Påmindelse ved optagelsens start";
    RECORDING_START_NOTICE_HINT = "Vis en påmindelse om at fortælle deltagerne, at mødet optages";
    RECORDING_DEVICES_TITLE = "Standardenheder til lyd";
    RECORDING_DEVICES_HELP = "Vælg den mikrofon og systemlyd, du helst vil optage med. De vælges automatisk, når du starter en ny optagelse.";
    RECORDING_DEVICES = "Lydenheder";
    RECORDING_MICROPHONE = "Mikrofon";
    RECORDING_MICROPHONE_DEFAULT = "Standardmikrofon";
    RECORDING_SYSTEM = "Systemlyd";
    RECORDING_SYSTEM_DEFAULT = "Standard for systemlyd";
    // GPUI: why the reminder and the devices cannot be set here.
    RECORDING_SETTINGS_LATER = "Påmindelsen og lydenhederne bruges, når du optager. Optagelse er endnu ikke flyttet til denne app, så vælg dem i den nuværende app.";
    RECORDING_NOTICE_LATER = "Påmindelsen og lydenhederne bruges, når du optager. Optagelse er endnu ikke flyttet til denne app, så vælg dem i den nuværende app. Kontakten viser standardværdien, ikke din gemte indstilling.";

    // Transskription: TranscriptSettings.tsx:101-128.
    TRANSCRIPTION_TITLE = "Transskription";
    TRANSCRIPTION_HELP = "Vælg, hvordan lyd bliver til tekst. Begge muligheder kører lokalt på din computer.";
    TRANSCRIPTION_ENGINE = "Transskriptionsmotor";
    TRANSCRIPTION_PARAKEET = "Parakeet (anbefalet, hurtig og præcis)";
    TRANSCRIPTION_WHISPER = "Whisper (lokal, høj nøjagtighed)";
    // TranscriptSettings.tsx:143 (model select placeholder).
    TRANSCRIPTION_MODEL_PLACEHOLDER = "Vælg model";
    // GPUI: the saved setting could not be read from the database.
    SAVED_SETTING_UNREADABLE = "Kunne ikke læses";
    // GPUI: the language group (#209) as a switch instead of a button.
    LANGUAGE_SUMMARY_SWITCH = "Skriv altid referat på hovedsproget";
    LANGUAGE_SUMMARY_SWITCH_HINT = "Slået fra følger referatet det mest talte af de valgte sprog.";
    // GPUI: the entry to Måling (docs/design-paritet.md 2.10).
    MEASURE_ENTRY_HELP = "Mål, hvor præcist de lokale modeller og dine udbydere skriver din egen stemme ned, før du vælger.";
    MEASURE_ENTRY = "Sammenlign modeller på din egen stemme";

    // Opsummering: SummaryModelSettings.tsx:125-148,
    // SummaryLanguageSettings.tsx:21-26 and 82, ModelSettingsModal.tsx:812-875.
    SUMMARY_AUTO = "Automatisk opsummering";
    SUMMARY_AUTO_HELP = "Lav automatisk en opsummering, når du stopper optagelsen af et møde.";
    SUMMARY_LANGUAGE = "Opsummeringssprog";
    SUMMARY_LANGUAGE_HELP = "Fastgør ét sprog som standard for nye møder. Sprog, du ikke har fastgjort, bliver ved med at være hurtigvalg, når du laver en opsummering. Automatisk bruger det sprog, der fylder mest i transskriptet.";
    SUMMARY_MODEL = "Model til opsummering";
    SUMMARY_MODEL_HELP = "Vælg den AI-model, der laver opsummeringer af dine møder.";
    SUMMARY_MODEL_PICK = "Vælg først en udbyder og derefter en model.";
    SUMMARY_PROVIDER_PLACEHOLDER = "Vælg udbyder";
    // GPUI: the accessible name of the provider field (Tauri has none).
    SUMMARY_PROVIDER = "Udbyder";
    // ModelSettingsModal.tsx:899.
    SUMMARY_MODEL_PLACEHOLDER = "Vælg model…";
    // ModelSettingsModal.tsx:878-884.
    SUMMARY_PROVIDER_BUILTIN = "Indbygget AI (offline, ingen API nødvendig)";
    SUMMARY_PROVIDER_CLAUDE = "Claude";
    SUMMARY_PROVIDER_CUSTOM = "Egen server (OpenAI-kompatibel)";
    SUMMARY_PROVIDER_GROQ = "Groq";
    SUMMARY_PROVIDER_OLLAMA = "Ollama";
    SUMMARY_PROVIDER_OPENAI = "OpenAI";
    SUMMARY_PROVIDER_OPENROUTER = "OpenRouter";
    // GPUI: why these cannot be set here yet.
    SUMMARY_AUTO_LATER = "Opsummeringen laves, når en optagelse stopper. Optagelse er endnu ikke flyttet til denne app. Kontakten viser standardværdien, ikke din gemte indstilling.";
    SUMMARY_LANGUAGE_LATER = "Hurtigvalg af sprog kommer i en senere version. Indtil da følger referatet indstillingerne under Transskription > Sprog.";
    SUMMARY_MODEL_LATER = "Valg af model er endnu ikke flyttet til denne app. Vælg modellen i den nuværende app.";

    // Beta: BetaSettings.tsx:23-68, types/betaFeatures.ts:35-42.
    BETA_BANNER_TITLE = "Betafunktioner";
    BETA_BANNER = "Disse funktioner er stadig under test. Du kan opleve fejl, og vi er glade for din tilbagemelding.";
    BETA_BADGE = "Beta";
    BETA_IMPORT = "Importér lyd og transskribér igen";
    BETA_IMPORT_HELP = "Importér lydfiler for at transskribere dem, eller transskribér eksisterende møder igen med andre sprogindstillinger.";
    BETA_INFO = "Når en betafunktion er slået fra, skjules den. Dine eksisterende møder berøres ikke.";
    // GPUI: import is not in this app yet (strings::IMPORT_LATER says the
    // same on the home screen).
    BETA_IMPORT_LATER = "Import er endnu ikke flyttet til denne app. Slå funktionen til eller fra i den nuværende app. Kontakten viser standardværdien, ikke din gemte indstilling.";
}

/// Tauri's name for a summary provider id (`ModelSettingsModal.tsx:878-884`);
/// an unknown id is shown as it is.
pub fn summary_provider(id: &str) -> &str {
    match id {
        "builtin-ai" => SUMMARY_PROVIDER_BUILTIN,
        "claude" => SUMMARY_PROVIDER_CLAUDE,
        "custom-openai" => SUMMARY_PROVIDER_CUSTOM,
        "groq" => SUMMARY_PROVIDER_GROQ,
        "ollama" => SUMMARY_PROVIDER_OLLAMA,
        "openai" => SUMMARY_PROVIDER_OPENAI,
        "openrouter" => SUMMARY_PROVIDER_OPENROUTER,
        other => other,
    }
}

/// Tauri's name for a transcription engine id (`TranscriptSettings.tsx:127-128`).
pub fn transcription_engine(id: &str) -> &str {
    match id {
        "parakeet" => TRANSCRIPTION_PARAKEET,
        "localWhisper" => TRANSCRIPTION_WHISPER,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ids_get_the_tauri_names_and_others_pass_through() {
        assert_eq!(summary_provider("ollama"), SUMMARY_PROVIDER_OLLAMA);
        assert_eq!(summary_provider("builtin-ai"), SUMMARY_PROVIDER_BUILTIN);
        assert_eq!(summary_provider("ny-udbyder"), "ny-udbyder");
        assert_eq!(transcription_engine("localWhisper"), TRANSCRIPTION_WHISPER);
        assert_eq!(transcription_engine("deepgram"), "deepgram");
    }
}
