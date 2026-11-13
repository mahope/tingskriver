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
    // DeviceSelection.tsx:225-365 (the device list and its messages).
    RECORDING_DEVICES_LOCKED = "Enhedsvalget er låst, mens en optagelse kører. Tilslutter du en ny enhed undervejs, skifter Tingskriver ikke til den. Stop optagelsen for at ændre enhed.";
    RECORDING_DEVICES_LOAD_FAILED = "Lydenhederne kunne ikke hentes. Tjek computerens lydindstillinger, og tryk på knappen for at hente listen igen.";
    RECORDING_DEVICES_LOADING = "Henter lydenheder…";
    RECORDING_DEVICES_REFRESH = "Hent listen over lydenheder igen";
    RECORDING_DEVICES_REFRESH_TOOLTIP = "Hent listen igen";
    RECORDING_NO_MICROPHONES = "Ingen mikrofoner fundet. Tilslut en mikrofon, og hent listen igen.";
    RECORDING_NO_SYSTEM = "Ingen enheder til systemlyd fundet. Hent listen igen, eller tjek computerens lydindstillinger.";
    RECORDING_INFO_MICROPHONE_LABEL = "Mikrofon:";
    RECORDING_INFO_MICROPHONE = "optager din stemme og lyden omkring dig";
    RECORDING_INFO_SYSTEM_LABEL = "Systemlyd:";
    RECORDING_INFO_SYSTEM = "optager lyd fra computeren (musik, opkald osv.)";
    // RecordingSettings.tsx:94-136 (the toasts).
    RECORDING_SETTING_SAVED = "Indstillingen er gemt";
    RECORDING_SETTING_SAVE_FAILED = "Indstillingen kunne ikke gemmes";
    RECORDING_SETTING_SAVE_FAILED_HELP = "Prøv igen. Hvis det fortsætter, så genstart Tingskriver.";
    RECORDING_DEVICES_SAVED = "Enhedsvalg gemt";
    RECORDING_DEVICES_SAVE_FAILED = "Enhedsvalget kunne ikke gemmes";
    RECORDING_DEVICES_DEFAULT_NAME = "Standard";
    // GPUI: the saved settings could not be read (Tauri falls back to the
    // default folder and keeps going).
    RECORDING_SETTINGS_UNREADABLE = "Dine gemte indstillinger for optagelse kunne ikke læses. Mappen og enhederne vises som standard.";
    // recordingNotification.tsx:24-50 (shown when a recording starts).
    RECORDING_REMINDER_TITLE = "Optagelsen er startet";
    RECORDING_REMINDER_TEXT = "Fortæl deltagerne, at mødet optages.";
    RECORDING_REMINDER_DONT_SHOW = "Vis ikke dette igen";
    RECORDING_REMINDER_ACK = "Deltagerne er informeret";
    // notifications/types.rs:116-135 (Notifikationer om start og slut).
    NOTIFY_APP = "Tingskriver";
    NOTIFY_STARTED = "Optagelsen er startet. Husk at fortælle de andre i mødet, at du optager.";
    NOTIFY_STOPPED = "Optagelsen er stoppet og gemt";
    NOTIFICATIONS_SAVE_FAILED = "Indstillingen kunne ikke gemmes. Prøv igen.";

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
    // GPUI: the saved model next to the read-only engine.
    TRANSCRIPTION_MODEL_CURRENT = "Valgt model: {value}";
    // GPUI: why the engine and the models cannot be chosen here.
    TRANSCRIPTION_ENGINE_LATER = "Valg af motor og modeller er endnu ikke flyttet til denne app. Skift motor eller hent modeller i den nuværende app.";
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
}

/// "Valgt model: …" under the read-only engine select.
pub fn transcription_model_current(model: &str) -> String {
    TRANSCRIPTION_MODEL_CURRENT.replace("{value}", model)
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

/// The toast after a device choice (`RecordingSettings.tsx:118-122`).
pub fn recording_devices_saved(microphone: Option<&str>, system: Option<&str>) -> String {
    format!(
        "Mikrofon: {}, systemlyd: {}",
        microphone.unwrap_or(RECORDING_DEVICES_DEFAULT_NAME),
        system.unwrap_or(RECORDING_DEVICES_DEFAULT_NAME)
    )
}

/// The toast text when the device choice could not be saved
/// (`RecordingSettings.tsx:127`).
pub fn recording_devices_save_failed(error: &str) -> String {
    format!("Prøv igen. {error}")
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
        assert_eq!(
            transcription_model_current("large-v3"),
            "Valgt model: large-v3"
        );
    }

    #[test]
    fn the_device_toast_names_defaults_as_tauri_does() {
        assert_eq!(
            recording_devices_saved(None, None),
            "Mikrofon: Standard, systemlyd: Standard"
        );
        assert_eq!(
            recording_devices_saved(Some("Headset (input)"), Some("Højttalere (output)")),
            "Mikrofon: Headset (input), systemlyd: Højttalere (output)"
        );
    }
}
