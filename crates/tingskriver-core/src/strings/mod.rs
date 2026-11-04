//! The Danish UI catalogue. Every constant and template is registered in ALL.
macro_rules! catalogue {
    ($($name:ident = $text:literal;)*) => {
        $(pub const $name: &str = $text;)*
        pub const ALL: &[&str] = &[$($name),*];
    };
}

catalogue! {
    UPDATE_TRAY = "Om Tingskriver · Opdatering klar";
    UPDATE_DETAILS = "Størrelse: {size} · Udgivet: {date}";
    UPDATE_NETWORK_FAILED = "Ingen forbindelse eller adgang til opdateringsfeedet. Kontrollér netværket og prøv igen.";
    UPDATE_DISK_FULL = "Der er ikke plads til opdateringen. Frigør diskplads, og prøv igen.";
    UPDATE_ACCESS_DENIED = "Tingskriver har ikke adgang til at gemme opdateringen. Kontrollér mappens rettigheder.";
    UPDATE_CHECK = "Søg efter opdateringer";
    UPDATE_DOWNLOAD = "Hent og installér";
    UPDATE_RESTART = "Genstart nu";
    UPDATE_LATER = "Genstart senere";
    UPDATE_IDLE = "Ingen ny opdatering fundet.";
    UPDATE_CHECKING = "Søger efter opdateringer …";
    UPDATE_AVAILABLE = "Version {version} kan hentes.";
    UPDATE_PROGRESS = "Henter version {version}: {percent} %";
    UPDATE_READY = "Version {version} er klar til genstart. Installeres ved næste start.";
    UPDATE_BLOCKED_DOWNLOAD = "Afslut optagelsen eller dikteringen, og vælg Hent og installér igen.";
    UPDATE_BLOCKED_RESTART = "Afslut optagelsen eller dikteringen, og vælg Genstart nu igen. Opdateringen er gemt.";
    UPDATE_DISABLED = "Opdateringstjek er slået fra i Indstillinger.";
    UPDATE_ENABLED_ON = "Automatisk opdateringstjek: Til";
    UPDATE_ENABLED_OFF = "Automatisk opdateringstjek: Fra";
    UPDATE_WEEKLY = "Hyppighed: Én gang om ugen";
    UPDATE_DAILY = "Hyppighed: Højst én gang i døgnet";
    UPDATE_PRIVACY = "Tjekker udgivelsesfeedet. Ingen telemetri. Hentning kræver dit valg på Om-siden.";
    UPDATE_BUSY = "Optagelse eller diktering er i gang. Hentning og genstart venter.";
    UPDATE_NO_NOTES = "Ingen ændringslog i feedet.";
    UPDATE_NO_DATE = "Dato ikke oplyst i feedet";
    UPDATE_SETTINGS_UNAVAILABLE = "Opdateringsindstillinger er ikke tilgængelige.";
    UPDATE_SETTINGS_INVALID = "Kunne ikke læse opdateringsindstillinger. Intet tjek udføres.";
    UPDATE_SETTINGS_FAILED = "Kunne ikke gemme opdateringsindstillinger.";
    UPDATE_SETTINGS_MISSING = "Indstillinger er ikke tilgængelige.";
    UPDATE_TIMESTAMP_FAILED = "Kunne ikke gemme tidspunkt for opdateringstjek. Intet tjek udføres.";
    UPDATE_START_FAILED = "Kunne ikke starte opdateringstjek.";
    UPDATE_VERIFY_FAILED = "Opdateringen kunne ikke verificeres. Hent den igen, eller kontakt support.";
    UPDATE_FEED_EMPTY = "Feedet indeholder ingen opdateringer til denne build. Prøv igen senere.";
    UPDATE_RECHECK = "Søg efter opdateringer igen.";
    UPDATE_FAILED = "Kunne ikke hente opdateringen. Kontrollér netværk, adgang til feedet og ledig diskplads, og prøv igen.";
    ABOUT = "Om";
    ABOUT_BUILD = "Byggedato";
    ABOUT_CHANNEL = "Kanal";
    ABOUT_STABLE = "stabil";
    ABOUT_BETA = "beta";
    ABOUT_PLATFORM = "Platform";
    ABOUT_UPDATES = "Opdateringer";
    ABOUT_UPDATES_UNAVAILABLE = "Opdateringer er ikke aktiveret i denne build";
    ABOUT_COPYING = "Kopierer diagnoseoplysninger …";
    ABOUT_COPIED = "Diagnoseoplysningerne er kopieret.";
    ABOUT_COPY_FAILED = "Kunne ikke kopiere diagnoseoplysninger. Prøv igen.";
    ABOUT_COPY = "Kopiér diagnoseoplysninger";
    ABOUT_GPU = "GPU-status";
    ABOUT_COMMIT = "Commit";
    ABOUT_MODELS = "Whisper: {whisper}\nParakeet: {parakeet}";
    ABOUT_MODEL_NOT_LOADED = "ikke indlæst";
    ABOUT_LICENSE = "Licens";
    ABOUT_THIRD_PARTY = "Tredjepartslicenser";
    ABOUT_NOTICE = "Tredjepartsattribution";
    ABOUT_PRIVACY = "Privatlivspolitik";
    ABOUT_CONTACT = "Kontakt: Mahope (mahoje.dk)";
    APP_NAME = "Tingskriver";
    MEETINGS = "Møder";
    DICTATION = "Diktering";
    SETTINGS = "Indstillinger";
    MEASURE_TITLE = "Måling";
    MEASURE_HELP = "Sammenlign dansk nøjagtighed på din egen stemme. Importér cirka 60 sekunder som WAV (mono, 16 kHz, PCM16, højst 90 sekunder), og skriv præcis det, du siger.";
    MEASURE_PATH = "Sti til lydprøve";
    MEASURE_PATH_HINT = "C:\u{005c}prøver\u{005c}prøve.wav";
    MEASURE_REFERENCE = "Facit";
    MEASURE_REFERENCE_HINT = "Skriv de ord, der bliver sagt i prøven.";
    MEASURE_MODELS = "Vælg udbydere og modeller";
    MEASURE_PRIVACY = "Ingen modeller er valgt på forhånd. Lyden sendes kun til de tjenester, du vælger. Hver model måles separat uden reserveudbyder. Facit sendes aldrig.";
    MEASURE_REMOTE = "modtager lyd over netværket";
    MEASURE_LOCAL = "kun på computeren";
    MEASURE_RATE = "Din tarif i DKK pr. lydtime (valgfri, uden strøm og hardware)";
    MEASURE_RATE_HINT = "Fx 12,50. Tomt betyder ukendt pris.";
    MEASURE_RELOAD = "Genindlæs udbyderindstillinger";
    MEASURE_SELECT = "Vælg:";
    MEASURE_SELECTED = "Valgt:";
    MEASURE_START = "Kør valgte modeller";
    MEASURE_RUNNING = "Måler … Vent, til alle valgte modeller er kørt.";
    MEASURE_INTERRUPTED = "Målingen blev afbrudt. Prøv igen.";
    MEASURE_SAVED = "Målingen er afsluttet. Resultatet er gemt lokalt.";
    MEASURE_RESULTS = "Resultater";
    MEASURE_MODEL_LABEL = "{provider}: {model} ({location})";
    MEASURE_ERRORS = "{value} ordfejl";
    MEASURE_WER = "WER {value} %";
    MEASURE_MS = "{value} ms";
    MEASURE_PRICE = "{value} {currency}/time";
    MEASURE_RESULT_LINE = "{wer}, {errors}, {latency}, {price}";
    MEASURE_PRICE_UNKNOWN = "pris ukendt";
    MEASURE_HISTORY = "Tidligere resultater ligger som JSON i datamappens measurements-mappe. Der gemmes kun måletal, ingen lyd eller facit. Svartid omfatter transskription og netværk, men ikke indlæsning af lokale modeller. Prisen er din angivne tarif, ikke en fakturamåling.";
    MEASURE_OPEN = "Åbn gemte resultater";
    MEASURE_NO_FOLDER = "Datamappen kunne ikke findes.";
    MEASURE_CHOOSE = "Vælg mindst én udbyder og model.";
    MEASURE_INVALID_RATE = "Angiv timeprisen som et tal, fx 12,50.";
    VERSION = "Version {value}";
    PROBE_LINE = "Skriv fx æøå, é, ü, ê, @ og € her";
    PROBE_TEXT = "Flere linjer: prøv også IME-komposition, fx Windows' emoji-panel (Win+.)";
    LOADING_MEETINGS = "Indlæser møder …";
    NO_MEETINGS = "Ingen møder endnu";
    FIRST_MEETING = "Når du har optaget dit første møde, står det her.";
    NO_MEETINGS_YET = "Ingen møder at vise endnu";
    MEETINGS_LATER = "Mødelisten kommer hertil i en senere version. Indtil da finder du dine møder i den nuværende app.";
    DICTATION_LATER = "Hold Ctrl+Win nede, tal, og slip. Teksten skrives der, hvor markøren står. Diktering flytter hertil i en senere version.";
    PROBE = "Prøvefelt";
    PROBE_HELP = "Brug felterne til at teste tastaturet: æøå, dødtaster (^ ¨ ~ ´ `) efterfulgt af en vokal, AltGr-tegn (@ £ $ € { [ ] } | \\) og IME-komposition.";
    SPECIAL_EMPTY = "Særlige tegn vises her med deres Unicode-nummer.";
    SPECIAL = "Særlige tegn: {value}";
    DARK = "mørk";
    LIGHT = "lys";
    UNKNOWN = "ukendt";
    DB_OPENING = "åbnes …";
    DB_NOT_CREATED = "ikke oprettet endnu";
    DB_READY = "klar";
    DB_FAILED = "kunne ikke åbnes";
    NO_LOG = "ingen logfil (kun konsollen)";
    SETTINGS_LATER = "Indstillingerne flytter hertil i en senere version. Her kan du se, hvordan appen kører.";
    APPEARANCE = "Udseende";
    APPEARANCE_DETAIL = "Følger Windows' indstilling for lys og mørk tilstand. Lige nu: {value}.";
    DATA = "Data";
    DATA_DIR = "Datamappe: {value}";
    DATABASE = "Database: {value}";
    LOG = "Log";
    LOG_FILE = "Logfil: {value}";
    OPEN_LOG = "Åbn logmappen";
    QUIT = "Afslut Tingskriver";
    LOG_FAILED = "Logfilen kunne ikke oprettes, så detaljerne står kun i konsollen.";
    DATABASE_FAILED = "Tingskriver kunne ikke åbne dine møder";
    RETRY = "Prøv igen";
}

macro_rules! parameterized {
    ($($function:ident => $template:ident;)*) => {
        $(pub fn $function(value: &str) -> String { $template.replace("{value}", value) })*
    };
}
parameterized! {
    measure_errors => MEASURE_ERRORS;
    measure_wer => MEASURE_WER;
    measure_ms => MEASURE_MS;
    version => VERSION;
    special_characters => SPECIAL;
    appearance => APPEARANCE_DETAIL;
    data_dir => DATA_DIR;
    database => DATABASE;
    log_file => LOG_FILE;
}

pub fn about_models(whisper: &str, parakeet: &str) -> String {
    ABOUT_MODELS
        .replace("{whisper}", whisper)
        .replace("{parakeet}", parakeet)
}

pub fn measure_model_label(provider: &str, model: &str, location: &str) -> String {
    MEASURE_MODEL_LABEL
        .replace("{provider}", provider)
        .replace("{model}", model)
        .replace("{location}", location)
}

pub fn measure_price(amount: &str, currency: &str) -> String {
    MEASURE_PRICE
        .replace("{value}", amount)
        .replace("{currency}", currency)
}

pub fn measure_result_line(wer: &str, errors: &str, latency: &str, price: &str) -> String {
    MEASURE_RESULT_LINE
        .replace("{wer}", wer)
        .replace("{errors}", errors)
        .replace("{latency}", latency)
        .replace("{price}", price)
}

#[cfg(test)]
mod tests {
    #[test]
    fn templates_replace_only_the_parameter() {
        assert_eq!(super::version("1.2.3"), "Version 1.2.3");
        assert_eq!(super::database("{value}"), "Database: {value}");
    }
}
